//! Discards (write-offs) from Search (4b): copies leave the stock without
//! counting as sold, and a stock-update CSV with negative quantities tells
//! Cardmarket (via TCG Powertools) to drop them too.
//!
//! Owner-confirmed rules (2026-11-15):
//! - A backup before every discard (`inventory.db.discard-<time>.bak`,
//!   newest [`KEEP_DISCARD_BACKUPS`]); no backup, no discard.
//! - The browser names piles and copies only; the server takes every value
//!   of the CSV from its own data, and refuses when fewer copies are left
//!   than asked for (in the pile and in the database), instead of quietly
//!   taking fewer.
//! - Discards stay *pending* until the next import, restore or desktop
//!   copy settles them: until then they are taken off the kept import file
//!   ([`crate::stock`]), and an export that still holds them is refused by
//!   the import check ([`returning_copies`]): it was downloaded before the
//!   CSV went into Cardmarket, and would bring the copies back and later
//!   count them as sold.
//! - The CSV of the newest [`KEEP_CSVS`] discards can be downloaded again.
//! - Discard, record and audit entry commit in one transaction.

use crate::audit::{self, action, Actor};
use crate::import::{prune_backups, unique_backup_path};
use crate::inventory_import::{Importer, MAX_QUANTITY};
use crate::migrations::format_utc;
use crate::stock::{kept_file_info, quantity, Source};
use check_stock_core::card_matching::MatchedCard;
use check_stock_core::formatters::format_update_stock_csv;
use check_stock_core::inventory_db::{
    card_variant_key, db_variant_key, db_variant_key_of, discard_cards_in_tx,
    pending_removals_conn, variant_quantity_conn,
};
use check_stock_core::models::Card;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// Most piles in one discard.
pub const MAX_ITEMS: usize = 1000;
/// Discards whose CSV is kept.
pub const KEEP_CSVS: i64 = 20;
/// Backups taken before discards that are kept.
pub const KEEP_DISCARD_BACKUPS: usize = 10;
/// Kind in the backup file names.
pub const BACKUP_KIND: &str = "discard";

/// One discarded pile, as recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscardedPile {
    pub pile: String,
    pub quantity: i64,
    pub cardmarket_id: String,
    pub name: String,
    pub set_code: String,
    pub condition: String,
    pub language: String,
    pub is_foil: String,
    pub is_signed: String,
    pub location: String,
    pub price: String,
}

impl DiscardedPile {
    fn of(pile: &str, card: &Card, quantity: i64) -> Self {
        Self {
            pile: pile.to_string(),
            quantity,
            cardmarket_id: card.cardmarket_id.clone(),
            name: card.name.clone(),
            set_code: card.set_code.clone(),
            condition: card.condition.clone(),
            language: card.language.clone(),
            is_foil: card.is_foil.clone(),
            is_signed: card.is_signed.clone(),
            location: card.location.clone().unwrap_or_default(),
            price: card.price.clone(),
        }
    }

    /// The database variant key of the pile.
    fn variant(&self) -> (String, String, String, String, String) {
        db_variant_key_of(
            &self.cardmarket_id,
            &self.condition,
            &self.language,
            &self.is_foil,
            &self.is_signed,
        )
    }
}

/// The pending (not yet settled) discards of `category`, oldest first.
pub fn pending(
    conn: &Connection,
    category: &str,
) -> rusqlite::Result<Vec<(i64, Vec<DiscardedPile>)>> {
    let mut stmt = conn.prepare(
        "SELECT id, items FROM srv_discards
         WHERE category = ?1 AND settled_at IS NULL ORDER BY id",
    )?;
    let rows = stmt.query_map([category], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (id, items) = row?;
        let items = serde_json::from_str(&items).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
        })?;
        Ok((id, items))
    })
    .collect()
}

/// Settles every pending discard (the inventory data was replaced by an
/// import, a restore or a desktop copy). Returns how many.
pub fn settle(conn: &Connection, now: DateTime<Utc>) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE srv_discards SET settled_at = ?1 WHERE settled_at IS NULL",
        [format_utc(now)],
    )
}

/// Copies discarded since the last import that an export of `category`
/// would bring back: per variant, the copies the file has above the
/// current stock, at most the discarded copies.
pub fn returning_copies(
    conn: &Connection,
    category: &str,
    file_cards: &[Card],
) -> rusqlite::Result<i64> {
    let discards = pending(conn, category)?;
    if discards.is_empty() {
        return Ok(0);
    }
    let mut discarded: HashMap<_, i64> = HashMap::new();
    for item in discards.iter().flat_map(|(_, items)| items) {
        *discarded.entry(item.variant()).or_default() += item.quantity;
    }
    let mut in_file: HashMap<_, i64> = HashMap::new();
    for card in file_cards {
        let key = db_variant_key(card);
        if discarded.contains_key(&key) {
            *in_file.entry(key).or_default() += quantity(card);
        }
    }
    let mut returning = 0;
    for (key, copies) in &discarded {
        let stock = variant_quantity_conn(conn, key, category)?.unwrap_or(0);
        let file = in_file.get(key).copied().unwrap_or(0);
        returning += (file - stock).max(0).min(*copies);
    }
    Ok(returning)
}

/// Why a discard was refused (nothing was written).
#[derive(Debug, Clone, PartialEq)]
pub enum DiscardError {
    Empty,
    TooMany,
    BadQuantity,
    /// A pile is not in the stock (any more).
    UnknownPile,
    /// Fewer copies left than asked for (the card's name, what is left).
    NotEnough {
        name: String,
        available: i64,
    },
    /// A new import came in since the search.
    Changed,
    BackupFailed,
}

/// A recorded discard.
#[derive(Debug, Clone, PartialEq)]
pub struct Discarded {
    pub id: i64,
    pub copies: i64,
    pub variants: i64,
    /// Listed value of the copies.
    pub value: f64,
}

/// Merges the request (pile id → copies) and checks it against `source`.
fn requested(source: &Source, items: &[(String, i64)]) -> Result<Vec<(String, i64)>, DiscardError> {
    if items.is_empty() {
        return Err(DiscardError::Empty);
    }
    if items.len() > MAX_ITEMS {
        return Err(DiscardError::TooMany);
    }
    let mut merged: Vec<(String, i64)> = Vec::new();
    for (pile, copies) in items {
        if !(1..=MAX_QUANTITY).contains(copies) {
            return Err(DiscardError::BadQuantity);
        }
        if source.pile(pile).is_none() {
            return Err(DiscardError::UnknownPile);
        }
        match merged.iter_mut().find(|(p, _)| p == pile) {
            Some((_, total)) => *total += copies,
            None => merged.push((pile.clone(), *copies)),
        }
    }
    Ok(merged)
}

/// Discards `items` (pile id, copies) of `source` for `user`: checks what is
/// left, takes the backup and records everything in one transaction
/// (writer connection; nothing else writes meanwhile).
pub fn discard(
    conn: &mut Connection,
    db_path: &Path,
    user: Importer<'_>,
    source: &Source,
    items: &[(String, i64)],
    now: DateTime<Utc>,
) -> rusqlite::Result<Result<Discarded, DiscardError>> {
    let merged = match requested(source, items) {
        Ok(merged) => merged,
        Err(e) => return Ok(Err(e)),
    };
    if kept_file_info(conn)?.map(|(version, _, _)| version) != Some(source.version) {
        return Ok(Err(DiscardError::Changed));
    }
    // Discards recorded after `source` was built take from the same piles.
    let mut later: HashMap<String, i64> = HashMap::new();
    for (id, piles) in pending(conn, crate::stock::CATEGORY)? {
        if id > source.discard_seq {
            for p in piles {
                *later.entry(p.pile).or_default() += p.quantity;
            }
        }
    }
    let mut piles: Vec<DiscardedPile> = Vec::new();
    let mut per_variant: HashMap<_, (i64, String, String)> = HashMap::new();
    for (pile, copies) in &merged {
        let card = source.pile(pile).expect("checked");
        let left = quantity(card) - later.get(pile).copied().unwrap_or(0);
        if *copies > left {
            return Ok(Err(DiscardError::NotEnough {
                name: card.name.clone(),
                available: left.max(0),
            }));
        }
        let entry = per_variant.entry(db_variant_key(card)).or_insert((
            0,
            card.name.clone(),
            card_variant_key(card, crate::stock::CATEGORY),
        ));
        entry.0 += copies;
        piles.push(DiscardedPile::of(pile, card, *copies));
    }
    // Copies promised to a bulk lot awaiting its import are not free.
    let mut reserved: HashMap<String, i64> = HashMap::new();
    for removal in pending_removals_conn(conn, crate::stock::CATEGORY)? {
        *reserved.entry(removal.variant).or_default() += removal.pending;
    }
    for (key, (copies, name, article)) in &per_variant {
        let stock = variant_quantity_conn(conn, key, crate::stock::CATEGORY)?.unwrap_or(0);
        let free = stock - reserved.get(article).copied().unwrap_or(0);
        if *copies > free {
            return Ok(Err(DiscardError::NotEnough {
                name: name.clone(),
                available: free.max(0),
            }));
        }
    }
    let backup = match backup_before_discard(conn, db_path) {
        Ok(path) => path,
        Err(e) => {
            log::error!("backup before the discard failed, nothing discarded: {e}");
            return Ok(Err(DiscardError::BackupFailed));
        }
    };

    let cards: Vec<(Card, i64)> = merged
        .iter()
        .map(|(pile, copies)| (source.pile(pile).expect("checked").clone(), *copies))
        .collect();
    let matched: Vec<MatchedCard> = cards
        .iter()
        .map(|(card, copies)| MatchedCard {
            card,
            quantity: i32::try_from(*copies).unwrap_or(i32::MAX),
            set_name: card.set.clone(),
        })
        .collect();
    let csv = format_update_stock_csv(&matched);
    let copies: i64 = merged.iter().map(|(_, c)| c).sum();
    let value: f64 = cards
        .iter()
        .map(|(card, c)| card.price_f64() * *c as f64)
        .sum();
    let variants = i64::try_from(per_variant.len()).unwrap_or(i64::MAX);

    let tx = conn.transaction()?;
    let stats = discard_cards_in_tx(&tx, &cards, crate::stock::CATEGORY)?;
    if stats.copies_discarded != copies {
        // Checked above on the same writer: cannot happen; never record
        // something else than what was taken.
        log::error!(
            "discard took {} copies instead of {copies}; rolled back",
            stats.copies_discarded
        );
        return Ok(Err(DiscardError::Changed));
    }
    tx.execute(
        "INSERT INTO srv_discards
             (category, file_version, created_at, user_id, user_name, copies, variants, items, csv)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            crate::stock::CATEGORY,
            source.version,
            format_utc(now),
            user.id,
            user.display_name,
            copies,
            variants,
            serde_json::to_string(&piles).expect("piles serialize"),
            csv.as_bytes()
        ],
    )?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "UPDATE srv_discards SET csv = NULL
         WHERE csv IS NOT NULL AND id NOT IN
             (SELECT id FROM srv_discards ORDER BY id DESC LIMIT ?1)",
        [KEEP_CSVS],
    )?;
    let backup_name = backup
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    audit::record(
        &tx,
        now,
        Actor::User {
            id: user.id,
            username: user.username,
        },
        action::INVENTORY_DISCARDED,
        Some(crate::stock::CATEGORY),
        serde_json::json!({
            "discard": id,
            "copies": copies,
            "variants": variants,
            "backup": backup_name,
            "piles": piles.iter().map(|p| serde_json::json!({
                "name": p.name,
                "set": p.set_code,
                "condition": p.condition,
                "language": p.language,
                "location": p.location,
                "copies": p.quantity,
            })).collect::<Vec<_>>(),
        }),
    )?;
    tx.commit()?;
    Ok(Ok(Discarded {
        id,
        copies,
        variants,
        value,
    }))
}

/// A consistent copy of the database next to it, before a discard; keeps
/// the newest [`KEEP_DISCARD_BACKUPS`]. Runs outside any transaction.
pub fn backup_before_discard(conn: &Connection, db_path: &Path) -> Result<PathBuf, String> {
    let stamp = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
    let target = unique_backup_path(db_path, BACKUP_KIND, &stamp);
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])
        .map_err(|e| e.to_string())?;
    if let Some(dir) = db_path.parent() {
        if let Err(e) = prune_backups(dir, BACKUP_KIND, KEEP_DISCARD_BACKUPS) {
            log::warn!("could not remove old discard backups: {e}");
        }
    }
    Ok(target)
}

/// One discard in the history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiscardSummary {
    pub id: i64,
    /// RFC 3339.
    pub created_at: String,
    pub user_name: String,
    pub copies: i64,
    pub variants: i64,
    /// Settled by a later import (or restore).
    pub settled: bool,
    /// Its CSV can still be downloaded.
    pub csv_available: bool,
}

/// The newest `limit` discards, newest first.
pub fn recent(conn: &Connection, limit: u32) -> rusqlite::Result<Vec<DiscardSummary>> {
    let mut stmt = conn.prepare(
        "SELECT id, created_at, user_name, copies, variants, settled_at IS NOT NULL,
                csv IS NOT NULL
         FROM srv_discards ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| {
        Ok(DiscardSummary {
            id: r.get(0)?,
            created_at: r.get(1)?,
            user_name: r.get(2)?,
            copies: r.get(3)?,
            variants: r.get(4)?,
            settled: r.get(5)?,
            csv_available: r.get(6)?,
        })
    })?;
    rows.collect()
}

/// The CSV of discard `id`, if it is still kept.
pub fn csv_of(conn: &Connection, id: i64) -> rusqlite::Result<Option<Vec<u8>>> {
    Ok(conn
        .query_row("SELECT csv FROM srv_discards WHERE id = ?1", [id], |r| {
            r.get::<_, Option<Vec<u8>>>(0)
        })
        .optional()?
        .flatten())
}

/// Newest discard id (0 before the first): part of the data version.
pub fn latest_id(conn: &Connection) -> rusqlite::Result<(i64, Option<String>)> {
    conn.query_row(
        "SELECT id, created_at FROM srv_discards ORDER BY id DESC LIMIT 1",
        [],
        |r| Ok((r.get(0)?, Some(r.get(1)?))),
    )
    .optional()
    .map(|v| v.unwrap_or((0, None)))
}

#[cfg(test)]
#[path = "discards_tests.rs"]
mod tests;
