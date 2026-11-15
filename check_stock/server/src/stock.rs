//! The stock as the screens that need every pile see it (search, stock
//! checker, picking, bin analysis): the confirmed file of the last inventory
//! import on the server (`srv_inventory_files`), minus the copies discarded
//! since (pending discards, see [`crate::discards`]).
//!
//! The inventory tables cannot be used for this: they keep one row per card
//! variant with the copies of all bins summed under one location.
//!
//! Every pile (row of the file) has an id: 16 hex characters of a SHA-256
//! over its card variant and location (an identical row gets `-2`, `-3`, …
//! in file order). Ids are computed on the file as imported, so discards do
//! not change them; they only have to be stable within one import, since
//! pending discards are settled by the next one.

use crate::db::{Db, DbError};
use crate::discards::{pending, DiscardedPile};
use crate::inventory_import::MAX_IMPORT_ROWS;
use check_stock_core::category::DEFAULT_CATEGORY;
use check_stock_core::io::parse_csv;
use check_stock_core::models::Card;
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

/// The category these screens work on (the Magic singles).
pub const CATEGORY: &str = DEFAULT_CATEGORY;

/// The stock of the last import, after the pending discards.
#[derive(Debug)]
pub struct Source {
    /// Inventory version of the import.
    pub version: i64,
    /// The newest pending discard taken off (0: none).
    pub discard_seq: i64,
    pub file_name: String,
    /// When it was imported (RFC 3339).
    pub imported_at: String,
    /// Piles with copies left.
    pub cards: Vec<Card>,
    /// The pile id of each card.
    pub ids: Vec<String>,
    index: HashMap<String, usize>,
}

impl Source {
    /// A source from cards with nothing discarded (tests, and the parsed
    /// file before discards).
    pub fn new(
        version: i64,
        discard_seq: i64,
        file_name: &str,
        imported_at: &str,
        cards: Vec<Card>,
    ) -> Self {
        let ids = pile_ids(&cards);
        Self::with_ids(version, discard_seq, file_name, imported_at, cards, ids)
    }

    fn with_ids(
        version: i64,
        discard_seq: i64,
        file_name: &str,
        imported_at: &str,
        cards: Vec<Card>,
        ids: Vec<String>,
    ) -> Self {
        let index = ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i))
            .collect();
        Self {
            version,
            discard_seq,
            file_name: file_name.to_string(),
            imported_at: imported_at.to_string(),
            cards,
            ids,
            index,
        }
    }

    /// The pile with `id`, if it still has copies.
    pub fn pile(&self, id: &str) -> Option<&Card> {
        self.index.get(id).map(|&i| &self.cards[i])
    }

    /// Whether `product` (Cardmarket id) is in this stock.
    pub fn has_product(&self, product: u64) -> bool {
        let id = product.to_string();
        self.cards.iter().any(|c| c.cardmarket_id.trim() == id)
    }
}

/// Copies of a pile (rows always have a positive whole quantity).
pub fn quantity(card: &Card) -> i64 {
    card.quantity.trim().parse().unwrap_or(0)
}

/// The id of every pile, in file order.
pub fn pile_ids(cards: &[Card]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    cards
        .iter()
        .map(|c| {
            let identity = [
                c.cardmarket_id.trim(),
                c.condition.trim(),
                c.language.trim(),
                c.is_foil.trim(),
                c.finish_type.trim(),
                c.is_signed.trim(),
                c.location.as_deref().unwrap_or("").trim(),
            ]
            .join("\u{1f}");
            let base: String = Sha256::digest(identity.as_bytes())[..8]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            let id = (1..)
                .map(|n| {
                    if n == 1 {
                        base.clone()
                    } else {
                        format!("{base}-{n}")
                    }
                })
                .find(|id| !seen.contains(id))
                .expect("a free id");
            seen.insert(id.clone());
            id
        })
        .collect()
}

/// `parsed` (the file as imported, with its ids) minus `discards`.
pub fn apply_discards(parsed: &Parsed, discards: &[(i64, Vec<DiscardedPile>)]) -> Source {
    let mut taken: HashMap<&str, i64> = HashMap::new();
    for (_, items) in discards {
        for item in items {
            *taken.entry(item.pile.as_str()).or_default() += item.quantity;
        }
    }
    let mut cards = Vec::with_capacity(parsed.cards.len());
    let mut ids = Vec::with_capacity(parsed.cards.len());
    for (card, id) in parsed.cards.iter().zip(&parsed.ids) {
        let left = quantity(card) - taken.get(id.as_str()).copied().unwrap_or(0);
        if left > 0 {
            let mut card = card.clone();
            card.quantity = left.to_string();
            cards.push(card);
            ids.push(id.clone());
        }
    }
    let seq = discards.iter().map(|(id, _)| *id).max().unwrap_or(0);
    Source::with_ids(
        parsed.version,
        seq,
        &parsed.file_name,
        &parsed.imported_at,
        cards,
        ids,
    )
}

/// The kept file as imported.
#[derive(Debug)]
pub struct Parsed {
    pub version: i64,
    pub file_name: String,
    pub imported_at: String,
    pub cards: Vec<Card>,
    pub ids: Vec<String>,
}

/// Version, name and time of the kept file of [`CATEGORY`].
pub fn kept_file_info(conn: &Connection) -> rusqlite::Result<Option<(i64, String, String)>> {
    conn.query_row(
        "SELECT f.version_id, f.file_name, v.created_at
         FROM srv_inventory_files f JOIN srv_inventory_versions v ON v.id = f.version_id
         WHERE f.category = ?1",
        [CATEGORY],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
}

/// Version, name, import time and bytes of a kept file.
type KeptFile = (i64, String, String, Vec<u8>);

fn kept_file(conn: &Connection) -> rusqlite::Result<Option<KeptFile>> {
    conn.query_row(
        "SELECT f.version_id, f.file_name, v.created_at, f.csv
         FROM srv_inventory_files f JOIN srv_inventory_versions v ON v.id = f.version_id
         WHERE f.category = ?1",
        [CATEGORY],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .optional()
}

#[derive(Debug)]
pub enum StockError {
    Db(DbError),
    /// The kept file no longer reads (it was checked at the import).
    Unreadable,
    Internal,
}

impl From<DbError> for StockError {
    fn from(e: DbError) -> Self {
        StockError::Db(e)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The parsed file (once per import) and the current source (once per
/// discard), shared by all users.
#[derive(Default)]
pub struct StockService {
    parsed: Mutex<Option<Arc<Parsed>>>,
    current: Mutex<Option<Arc<Source>>>,
}

impl StockService {
    /// The current stock; `None` before the first import on the server.
    pub async fn source(&self, db: &Db) -> Result<Option<Arc<Source>>, StockError> {
        let state = db
            .read(|conn| {
                let Some((version, _, _)) = kept_file_info(conn)? else {
                    return Ok(None);
                };
                Ok(Some((version, pending(conn, CATEGORY)?)))
            })
            .await?;
        let Some((version, discards)) = state else {
            return Ok(None);
        };
        let seq = discards.iter().map(|(id, _)| *id).max().unwrap_or(0);
        if let Some(cached) = lock(&self.current).as_ref() {
            if cached.version == version && cached.discard_seq == seq {
                return Ok(Some(cached.clone()));
            }
        }
        let parsed = self.parsed(db, version).await?;
        let Some(parsed) = parsed else {
            return Ok(None);
        };
        let source =
            tokio::task::spawn_blocking(move || Arc::new(apply_discards(&parsed, &discards)))
                .await
                .map_err(|_| StockError::Internal)?;
        *lock(&self.current) = Some(source.clone());
        Ok(Some(source))
    }

    async fn parsed(&self, db: &Db, version: i64) -> Result<Option<Arc<Parsed>>, StockError> {
        if let Some(cached) = lock(&self.parsed).as_ref() {
            if cached.version == version {
                return Ok(Some(cached.clone()));
            }
        }
        let Some((version, file_name, imported_at, csv)) = db.read(kept_file).await? else {
            return Ok(None);
        };
        let parsed = tokio::task::spawn_blocking(move || {
            let cards = parse_csv(&csv, MAX_IMPORT_ROWS).map_err(|e| {
                log::error!("the kept inventory file of version {version} no longer reads: {e}");
                StockError::Unreadable
            })?;
            let ids = pile_ids(&cards);
            Ok::<_, StockError>(Arc::new(Parsed {
                version,
                file_name,
                imported_at,
                cards,
                ids,
            }))
        })
        .await
        .map_err(|_| StockError::Internal)??;
        *lock(&self.parsed) = Some(parsed.clone());
        Ok(Some(parsed))
    }
}

#[cfg(test)]
#[path = "stock_tests.rs"]
mod tests;
