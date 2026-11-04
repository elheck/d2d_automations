//! Bulk lots: old, cheap singles sold together as one Cardmarket lot
//! ("100 commons").
//!
//! Building a lot records two things:
//!
//! - **Revenue.** The lot price is booked on the day the lot is built and split
//!   evenly across its copies; each copy credits the purchase lot (`L#`) it
//!   came from. It counts towards money totals (purchase-lot revenue, daily
//!   snapshots, the welcome digest) but never towards per-card sales signals
//!   (`sold_events`, `sold_quantity`, restock candidates): one cent per common
//!   says nothing about what the card sells for as a single.
//! - **Stock.** The copies are *not* taken out of `inventory_cards` right away.
//!   Each one becomes a pending removal, and the next sync that sees the copies
//!   gone (after the TCG Powertools CSV was imported and a fresh export loaded)
//!   uses the pending removals up before recording anything as sold. So it
//!   never matters whether the Powertools import or the next sync comes first.

use super::{
    article_key, canonical_condition, card_lot, normalize_flag, normalize_language, open_db,
    parse_csv_price, today_date, DbResult,
};
use crate::models::Card;
use rusqlite::{params, Connection};
use std::collections::HashMap;

pub(super) const BULK_LOTS_DDL: &str = "
    CREATE TABLE IF NOT EXISTS bulk_lots (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        name       TEXT NOT NULL,
        created_at TEXT NOT NULL,
        price      REAL NOT NULL
    );
    CREATE TABLE IF NOT EXISTS bulk_lot_cards (
        id             INTEGER PRIMARY KEY AUTOINCREMENT,
        bulk_lot_id    INTEGER NOT NULL,
        category       TEXT NOT NULL,
        cardmarket_id  TEXT NOT NULL,
        condition      TEXT NOT NULL,
        language       TEXT NOT NULL,
        is_foil        TEXT NOT NULL,
        is_signed      TEXT NOT NULL,
        name           TEXT NOT NULL,
        set_name       TEXT NOT NULL,
        rarity         TEXT NOT NULL,
        foil           INTEGER NOT NULL,
        location       TEXT NOT NULL,
        purchase_lot   TEXT NOT NULL,
        copies         INTEGER NOT NULL,
        listed_price   REAL NOT NULL,
        pending_copies INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_bulk_lot_cards_lot ON bulk_lot_cards (bulk_lot_id);
    CREATE INDEX IF NOT EXISTS idx_bulk_lot_cards_variant
        ON bulk_lot_cards (category, cardmarket_id, condition, language, is_foil, is_signed);
";

/// Longest accepted lot name, in characters.
pub const MAX_LOT_NAME_CHARS: usize = 120;
/// Highest accepted lot price, in EUR — a guard against typos (`10000` for `100`).
pub const MAX_LOT_PRICE_EUR: f64 = 100_000.0;
/// Most copies one lot may contain.
pub const MAX_LOT_COPIES: i64 = 10_000;

/// One built bulk lot, as listed in the history.
#[derive(Debug, Clone, PartialEq)]
pub struct BulkLotSummary {
    pub id: i64,
    pub name: String,
    /// Day the lot was built and its revenue booked (`YYYY-MM-DD`).
    pub created_at: String,
    /// Sale price of the whole lot, in EUR.
    pub price: f64,
    pub copies: i64,
    /// Copies no sync has seen leave the inventory yet.
    pub pending_copies: i64,
    /// What the copies were listed at as singles (Σ listed price × copies).
    pub listed_value: f64,
}

impl BulkLotSummary {
    /// Revenue booked per copy (the lot price split evenly).
    pub fn per_card(&self) -> f64 {
        per_card_share(self.price, self.copies)
    }
}

/// Lot price split evenly across `copies`; `0.0` for an empty lot.
pub fn per_card_share(price: f64, copies: i64) -> f64 {
    if copies > 0 {
        price / copies as f64
    } else {
        0.0
    }
}

/// One line of a built lot (copies of one variant from one location).
#[derive(Debug, Clone, PartialEq)]
pub struct BulkLotCard {
    pub name: String,
    pub set_name: String,
    pub rarity: String,
    pub condition: String,
    pub language: String,
    pub foil: bool,
    pub location: String,
    pub purchase_lot: String,
    pub copies: i64,
    pub pending_copies: i64,
    pub listed_price: f64,
}

/// Copies of a bulk lot that the inventory has not been seen without yet.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingRemoval {
    /// Ledger row id.
    pub id: i64,
    pub bulk_lot_id: i64,
    /// Article key of the variant (see [`super::card_variant_key`]).
    pub variant: String,
    /// Purchase lot the copies came from (`""` when the location names none).
    pub purchase_lot: String,
    pub location: String,
    pub pending: i64,
}

/// Totals over a set of bulk lots.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BulkLotTotals {
    pub lots: i64,
    pub copies: i64,
    pub revenue: f64,
}

fn invalid(message: String) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(message.into())
}

/// Trimmed lot name, or an error when it is empty or too long.
fn validated_name(name: &str) -> DbResult<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(invalid("the lot needs a name".to_string()));
    }
    if name.chars().count() > MAX_LOT_NAME_CHARS {
        return Err(invalid(format!(
            "the lot name is longer than {MAX_LOT_NAME_CHARS} characters"
        )));
    }
    Ok(name)
}

fn validate_price(price: f64) -> DbResult<()> {
    if price.is_finite() && (0.0..=MAX_LOT_PRICE_EUR).contains(&price) {
        Ok(())
    } else {
        Err(invalid(format!(
            "the lot price must be between €0 and €{MAX_LOT_PRICE_EUR:.0}, got {price}"
        )))
    }
}

/// Records a built lot: books `price` as revenue today and registers every
/// copy as a pending removal. `cards` are inventory CSV rows with the number
/// of copies taken from each. Returns the new lot's id.
pub fn record_bulk_lot(
    name: &str,
    price: f64,
    cards: &[(Card, i64)],
    category: &str,
) -> DbResult<i64> {
    let mut conn = open_db()?;
    record_bulk_lot_conn(&mut conn, name, price, cards, category, &today_date())
}

/// Inner write that accepts an explicit connection and date — used in tests.
pub(super) fn record_bulk_lot_conn(
    conn: &mut Connection,
    name: &str,
    price: f64,
    cards: &[(Card, i64)],
    category: &str,
    today: &str,
) -> DbResult<i64> {
    let name = validated_name(name)?;
    validate_price(price)?;
    if cards.is_empty() {
        return Err(invalid("the lot contains no cards".to_string()));
    }
    if let Some((card, copies)) = cards.iter().find(|(_, copies)| *copies <= 0) {
        return Err(invalid(format!(
            "{copies} copies of {} — every line needs at least one copy",
            card.name
        )));
    }
    let total: i64 = cards.iter().map(|(_, copies)| copies).sum();
    if total > MAX_LOT_COPIES {
        return Err(invalid(format!(
            "{total} copies in one lot; at most {MAX_LOT_COPIES} are allowed"
        )));
    }

    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO bulk_lots (name, created_at, price) VALUES (?1, ?2, ?3)",
        params![name, today, price],
    )?;
    let lot_id = tx.last_insert_rowid();
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO bulk_lot_cards
                (bulk_lot_id, category, cardmarket_id, condition, language, is_foil, is_signed,
                 name, set_name, rarity, foil, location, purchase_lot, copies, listed_price,
                 pending_copies)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?14)",
        )?;
        for (card, copies) in cards {
            stmt.execute(params![
                lot_id,
                category,
                card.cardmarket_id,
                canonical_condition(&card.condition),
                normalize_language(&card.language),
                normalize_flag(&card.is_foil),
                normalize_flag(&card.is_signed),
                card.name,
                card.set,
                card.rarity,
                card.is_foil_card(),
                card.location.as_deref().unwrap_or("").trim(),
                card_lot(card),
                copies,
                parse_csv_price(&card.price).unwrap_or(0.0),
            ])?;
        }
    }
    tx.commit()?;
    log::info!("Bulk lot #{lot_id} '{name}' recorded: {total} copies for €{price:.2}");
    Ok(lot_id)
}

/// All built lots, newest first.
pub fn list_bulk_lots() -> DbResult<Vec<BulkLotSummary>> {
    let conn = open_db()?;
    list_bulk_lots_conn(&conn)
}

pub(super) fn list_bulk_lots_conn(conn: &Connection) -> DbResult<Vec<BulkLotSummary>> {
    conn.prepare(
        "SELECT l.id, l.name, l.created_at, l.price,
                COALESCE(SUM(c.copies), 0),
                COALESCE(SUM(c.pending_copies), 0),
                COALESCE(SUM(c.copies * c.listed_price), 0.0)
         FROM bulk_lots l
         LEFT JOIN bulk_lot_cards c ON c.bulk_lot_id = l.id
         GROUP BY l.id
         ORDER BY l.created_at DESC, l.id DESC",
    )?
    .query_map([], |r| {
        Ok(BulkLotSummary {
            id: r.get(0)?,
            name: r.get(1)?,
            created_at: r.get(2)?,
            price: r.get(3)?,
            copies: r.get(4)?,
            pending_copies: r.get(5)?,
            listed_value: r.get(6)?,
        })
    })?
    .collect()
}

/// The cards of one lot, in picking order (by location).
pub fn get_bulk_lot_cards(lot_id: i64) -> DbResult<Vec<BulkLotCard>> {
    let conn = open_db()?;
    bulk_lot_cards_conn(&conn, lot_id)
}

pub(super) fn bulk_lot_cards_conn(conn: &Connection, lot_id: i64) -> DbResult<Vec<BulkLotCard>> {
    conn.prepare(
        "SELECT name, set_name, rarity, condition, language, foil, location, purchase_lot,
                copies, pending_copies, listed_price
         FROM bulk_lot_cards WHERE bulk_lot_id = ?1
         ORDER BY location, name, id",
    )?
    .query_map(params![lot_id], |r| {
        Ok(BulkLotCard {
            name: r.get(0)?,
            set_name: r.get(1)?,
            rarity: r.get(2)?,
            condition: r.get(3)?,
            language: r.get(4)?,
            foil: r.get(5)?,
            location: r.get(6)?,
            purchase_lot: r.get(7)?,
            copies: r.get(8)?,
            pending_copies: r.get(9)?,
            listed_price: r.get(10)?,
        })
    })?
    .collect()
}

/// Corrects the price a lot sold for. Its per-copy revenue follows, since it
/// is always derived from the lot price.
pub fn set_bulk_lot_price(lot_id: i64, price: f64) -> DbResult<()> {
    let conn = open_db()?;
    set_bulk_lot_price_conn(&conn, lot_id, price)
}

pub(super) fn set_bulk_lot_price_conn(conn: &Connection, lot_id: i64, price: f64) -> DbResult<()> {
    validate_price(price)?;
    match conn.execute(
        "UPDATE bulk_lots SET price = ?1 WHERE id = ?2",
        params![price, lot_id],
    )? {
        0 => Err(rusqlite::Error::QueryReturnedNoRows),
        _ => Ok(()),
    }
}

/// Declares a lot's removals done without waiting for a sync to see them —
/// for when the inventory already reflects them but no sync could tell (for
/// example because the same card was restocked at the same time).
pub fn mark_bulk_lot_removed(lot_id: i64) -> DbResult<()> {
    let conn = open_db()?;
    mark_bulk_lot_removed_conn(&conn, lot_id)
}

pub(super) fn mark_bulk_lot_removed_conn(conn: &Connection, lot_id: i64) -> DbResult<()> {
    conn.execute(
        "UPDATE bulk_lot_cards SET pending_copies = 0 WHERE bulk_lot_id = ?1",
        params![lot_id],
    )?;
    Ok(())
}

/// Deletes a lot with its revenue and pending removals. Copies whose removal
/// no sync has seen yet count as ordinary stock again.
pub fn delete_bulk_lot(lot_id: i64) -> DbResult<()> {
    let mut conn = open_db()?;
    delete_bulk_lot_conn(&mut conn, lot_id)
}

pub(super) fn delete_bulk_lot_conn(conn: &mut Connection, lot_id: i64) -> DbResult<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM bulk_lot_cards WHERE bulk_lot_id = ?1",
        params![lot_id],
    )?;
    let deleted = tx.execute("DELETE FROM bulk_lots WHERE id = ?1", params![lot_id])?;
    if deleted == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    tx.commit()?;
    log::info!("Bulk lot #{lot_id} deleted");
    Ok(())
}

/// Pending removals of one category, oldest lot first.
pub fn get_pending_bulk_removals(category: &str) -> DbResult<Vec<PendingRemoval>> {
    let conn = open_db()?;
    pending_removals_conn(&conn, category)
}

pub(super) fn pending_removals_conn(
    conn: &Connection,
    category: &str,
) -> DbResult<Vec<PendingRemoval>> {
    conn.prepare(
        "SELECT id, bulk_lot_id, cardmarket_id, condition, language, is_foil, is_signed,
                purchase_lot, location, pending_copies
         FROM bulk_lot_cards
         WHERE category = ?1 AND pending_copies > 0
         ORDER BY bulk_lot_id ASC, id ASC",
    )?
    .query_map(params![category], |r| {
        Ok(PendingRemoval {
            id: r.get(0)?,
            bulk_lot_id: r.get(1)?,
            variant: article_key(
                category,
                &r.get::<_, String>(2)?,
                &r.get::<_, String>(3)?,
                &r.get::<_, String>(4)?,
                &r.get::<_, String>(5)?,
                &r.get::<_, String>(6)?,
            ),
            purchase_lot: r.get(7)?,
            location: r.get(8)?,
            pending: r.get(9)?,
        })
    })?
    .collect()
}

/// When each variant of a category was listed: `listed_at`, or the first sync
/// that saw it when Cardmarket's export carries no listing date. Keyed by
/// article key; used to tell how long a card has sat in stock.
pub fn get_listing_dates(category: &str) -> DbResult<HashMap<String, String>> {
    let conn = open_db()?;
    listing_dates_conn(&conn, category)
}

pub(super) fn listing_dates_conn(
    conn: &Connection,
    category: &str,
) -> DbResult<HashMap<String, String>> {
    conn.prepare(
        "SELECT cardmarket_id, condition, language, is_foil, is_signed,
                COALESCE(NULLIF(listed_at, ''), first_synced_at)
         FROM inventory_cards WHERE category = ?1",
    )?
    .query_map(params![category], |r| {
        Ok((
            article_key(
                category,
                &r.get::<_, String>(0)?,
                &r.get::<_, String>(1)?,
                &r.get::<_, String>(2)?,
                &r.get::<_, String>(3)?,
                &r.get::<_, String>(4)?,
            ),
            r.get::<_, String>(5)?,
        ))
    })?
    .collect()
}

/// Bulk-lot revenue per purchase lot: `lot → (copies, revenue)`. Each copy
/// earns its lot's price split evenly across the lot's copies.
pub(super) fn revenue_by_purchase_lot(conn: &Connection) -> DbResult<HashMap<String, (i64, f64)>> {
    conn.prepare(
        "SELECT c.purchase_lot, SUM(c.copies), SUM(c.copies * l.price / t.total)
         FROM bulk_lot_cards c
         JOIN bulk_lots l ON l.id = c.bulk_lot_id
         JOIN (SELECT bulk_lot_id, SUM(copies) AS total
               FROM bulk_lot_cards GROUP BY bulk_lot_id) t ON t.bulk_lot_id = c.bulk_lot_id
         WHERE t.total > 0
         GROUP BY c.purchase_lot",
    )?
    .query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            (r.get::<_, i64>(1)?, r.get::<_, f64>(2)?),
        ))
    })?
    .collect()
}

/// Totals over the lots built in `(after, until]` — either bound optional.
pub(super) fn totals_between(
    conn: &Connection,
    after: Option<&str>,
    until: Option<&str>,
) -> DbResult<BulkLotTotals> {
    conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(l.price), 0.0), COALESCE(SUM(t.copies), 0)
         FROM bulk_lots l
         LEFT JOIN (SELECT bulk_lot_id, SUM(copies) AS copies
                    FROM bulk_lot_cards GROUP BY bulk_lot_id) t ON t.bulk_lot_id = l.id
         WHERE (?1 IS NULL OR l.created_at > ?1) AND (?2 IS NULL OR l.created_at <= ?2)",
        params![after, until],
        |r| {
            Ok(BulkLotTotals {
                lots: r.get(0)?,
                revenue: r.get(1)?,
                copies: r.get(2)?,
            })
        },
    )
}

/// How one sync's quantity drops are explained by pending bulk-lot removals.
///
/// Pending copies are used oldest lot first, and only as far as the variant
/// actually dropped: a variant whose stock did not drop (CSV not imported
/// into Cardmarket yet) keeps its removals pending for a later sync.
#[derive(Debug, Default)]
pub(super) struct RemovalPlan {
    /// Ledger row id → copies used up by this sync.
    consumed: Vec<(i64, i64)>,
    /// Article key → copies explained by removals.
    per_variant: HashMap<String, i64>,
    /// Article key → purchase lot → copies explained by removals.
    per_lot: HashMap<String, HashMap<String, i64>>,
}

impl RemovalPlan {
    /// Plans removals for the given per-variant drops (`article key → copies`).
    pub(super) fn plan(drops: &HashMap<String, i64>, pending: &[PendingRemoval]) -> Self {
        let mut plan = Self::default();
        for removal in pending {
            let dropped = drops.get(&removal.variant).copied().unwrap_or(0);
            let used = plan.removed(&removal.variant);
            let take = removal.pending.min(dropped - used);
            if take <= 0 {
                continue;
            }
            plan.consumed.push((removal.id, take));
            *plan.per_variant.entry(removal.variant.clone()).or_insert(0) += take;
            *plan
                .per_lot
                .entry(removal.variant.clone())
                .or_default()
                .entry(removal.purchase_lot.clone())
                .or_insert(0) += take;
        }
        plan
    }

    /// Copies of `variant` this sync attributes to bulk lots.
    pub(super) fn removed(&self, variant: &str) -> i64 {
        self.per_variant.get(variant).copied().unwrap_or(0)
    }

    /// Those copies split by the purchase lot they came from.
    pub(super) fn removed_by_lot(&self, variant: &str) -> Option<&HashMap<String, i64>> {
        self.per_lot.get(variant)
    }

    /// All copies this sync attributes to bulk lots.
    pub(super) fn total(&self) -> i64 {
        self.per_variant.values().sum()
    }

    /// Marks the planned copies as no longer pending.
    pub(super) fn apply(&self, conn: &Connection) -> DbResult<()> {
        let mut stmt = conn.prepare_cached(
            "UPDATE bulk_lot_cards SET pending_copies = pending_copies - ?1 WHERE id = ?2",
        )?;
        for (id, copies) in &self.consumed {
            stmt.execute(params![copies, id])?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "bulk_lots_tests.rs"]
mod tests;
