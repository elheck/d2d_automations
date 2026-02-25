//! Local SQLite database for inventory sync.
//!
//! Every time a Cardmarket inventory CSV is loaded, the cards are synced here.
//! - Articles no longer in the CSV have their quantity set to 0 (never deleted).
//! - `first_synced_at`: set once on first insert, never changed.
//! - `last_synced_at`: updated to today on each sync, max once per day.
//! - Articles already at quantity 0 and not in the CSV are left untouched.
//! - Multiple CSV rows for the same card variant (same condition/language/foil/signed)
//!   in different physical locations are merged: quantities are summed, one DB row kept.

use crate::models::Card;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::PathBuf;

/// Result type for database operations
pub type DbResult<T> = Result<T, rusqlite::Error>;

/// One row in the "longest unsold" top-5 list.
#[derive(Debug)]
pub struct OldestInStockEntry {
    pub name: String,
    /// Effective date: `listed_at` when available, otherwise `first_synced_at`.
    pub date: String,
    pub price: f64,
    pub quantity: i64,
    pub location: String,
}

/// Aggregate statistics about the current inventory database contents.
#[derive(Debug, Default)]
pub struct DbStats {
    /// Total number of article rows in the database (includes sold-out qty=0 listings)
    pub total_articles: i64,
    /// Articles currently in stock (quantity > 0)
    pub in_stock_articles: i64,
    /// Total copies across all in-stock listings
    pub total_copies: i64,
    /// Sum of (price × quantity) for all in-stock cards, in EUR
    pub total_value: f64,
    /// Number of foil card listings with quantity > 0
    pub foil_count: i64,
    /// Number of signed card listings with quantity > 0
    pub signed_count: i64,
    /// Top 5 cards by total copies: (name, total_quantity)
    pub top_by_quantity: Vec<(String, i64)>,
    /// Top 5 most expensive individual listings: (name, price)
    pub top_by_price: Vec<(String, f64)>,
    /// Top 5 cards that have been in stock the longest without selling.
    pub top_oldest_in_stock: Vec<OldestInStockEntry>,
    /// Oldest in-stock card by listed_at date: (name, date)
    pub oldest_listed: Option<(String, String)>,
    /// Most recently listed in-stock card: (name, date)
    pub newest_listed: Option<(String, String)>,
    /// Date of the very first sync recorded in the database
    pub first_synced_date: Option<String>,
    /// Total copies by language, sorted by count descending
    pub language_breakdown: Vec<(String, i64)>,
    /// Total copies by condition, sorted by count descending
    pub condition_breakdown: Vec<(String, i64)>,
    /// Total copies by rarity, sorted by count descending
    pub rarity_breakdown: Vec<(String, i64)>,
}

/// Statistics from a sync operation
#[derive(Debug, Default)]
pub struct SyncStats {
    /// Number of unique card variants from the CSV that were inserted or updated
    pub upserted: usize,
    /// Number of card variants no longer in the CSV that were set to quantity 0
    pub zeroed: usize,
}

/// Returns the path to the inventory database file.
fn db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("d2d_automations")
        .join("inventory.db")
}

/// Opens (or creates) the inventory database and initialises the schema.
fn open_db() -> DbResult<Connection> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    log::info!("Inventory DB: {}", path.display());
    let conn = Connection::open(&path)?;
    init_schema(&conn)?;
    Ok(conn)
}

// DDL for the current schema (v2): one row per unique card variant.
// `cardmarketId` is a Cardmarket product ID shared across all language/condition
// variants of the same card — NOT unique per article. The composite key
// (cardmarket_id, condition, language, is_foil, is_signed) identifies a variant.
// Multiple physical locations of the same variant are merged: quantities are summed.
const INVENTORY_CARDS_DDL: &str = "
    CREATE TABLE inventory_cards (
        cardmarket_id   TEXT NOT NULL,
        quantity        INTEGER NOT NULL,
        name            TEXT NOT NULL,
        set_name        TEXT NOT NULL,
        set_code        TEXT NOT NULL,
        cn              TEXT NOT NULL,
        condition       TEXT NOT NULL,
        language        TEXT NOT NULL,
        is_foil         TEXT NOT NULL,
        is_playset      TEXT,
        is_signed       TEXT NOT NULL,
        price           TEXT NOT NULL,
        comment         TEXT NOT NULL,
        location        TEXT,
        name_de         TEXT NOT NULL,
        name_es         TEXT NOT NULL,
        name_fr         TEXT NOT NULL,
        name_it         TEXT NOT NULL,
        rarity          TEXT NOT NULL,
        listed_at       TEXT NOT NULL,
        first_synced_at TEXT NOT NULL,
        last_synced_at  TEXT NOT NULL
    );
    CREATE UNIQUE INDEX idx_inventory_article_key
        ON inventory_cards (cardmarket_id, condition, language, is_foil, is_signed);
";

// Migration v1 → v2: replace single cardmarket_id PRIMARY KEY with composite UNIQUE key.
const MIGRATION_V1_TO_V2: &str = "
    BEGIN;
    CREATE TABLE inventory_cards_v2 (
        cardmarket_id   TEXT NOT NULL,
        quantity        INTEGER NOT NULL,
        name            TEXT NOT NULL,
        set_name        TEXT NOT NULL,
        set_code        TEXT NOT NULL,
        cn              TEXT NOT NULL,
        condition       TEXT NOT NULL,
        language        TEXT NOT NULL,
        is_foil         TEXT NOT NULL,
        is_playset      TEXT,
        is_signed       TEXT NOT NULL,
        price           TEXT NOT NULL,
        comment         TEXT NOT NULL,
        location        TEXT,
        name_de         TEXT NOT NULL,
        name_es         TEXT NOT NULL,
        name_fr         TEXT NOT NULL,
        name_it         TEXT NOT NULL,
        rarity          TEXT NOT NULL,
        listed_at       TEXT NOT NULL,
        first_synced_at TEXT NOT NULL,
        last_synced_at  TEXT NOT NULL
    );
    INSERT OR IGNORE INTO inventory_cards_v2
        SELECT cardmarket_id, quantity, name, set_name, set_code, cn,
               condition, language, is_foil, is_playset, is_signed,
               price, comment, location, name_de, name_es, name_fr, name_it,
               rarity, listed_at, first_synced_at, last_synced_at
        FROM inventory_cards;
    DROP TABLE inventory_cards;
    ALTER TABLE inventory_cards_v2 RENAME TO inventory_cards;
    CREATE UNIQUE INDEX idx_inventory_article_key
        ON inventory_cards (cardmarket_id, condition, language, is_foil, is_signed);
    COMMIT;
";

// Migration v3 → v2: collapse per-location rows back into one row per card variant.
// v3 stored one row per physical location (6-field key including location). v2 stores
// one row per variant (5-field key), with quantities summed across all locations.
// Keeps the earliest first_synced_at and latest last_synced_at across merged rows.
const MIGRATION_V3_TO_V2: &str = "
    BEGIN;
    CREATE TABLE inventory_cards_merged (
        cardmarket_id   TEXT NOT NULL,
        quantity        INTEGER NOT NULL,
        name            TEXT NOT NULL,
        set_name        TEXT NOT NULL,
        set_code        TEXT NOT NULL,
        cn              TEXT NOT NULL,
        condition       TEXT NOT NULL,
        language        TEXT NOT NULL,
        is_foil         TEXT NOT NULL,
        is_playset      TEXT,
        is_signed       TEXT NOT NULL,
        price           TEXT NOT NULL,
        comment         TEXT NOT NULL,
        location        TEXT,
        name_de         TEXT NOT NULL,
        name_es         TEXT NOT NULL,
        name_fr         TEXT NOT NULL,
        name_it         TEXT NOT NULL,
        rarity          TEXT NOT NULL,
        listed_at       TEXT NOT NULL,
        first_synced_at TEXT NOT NULL,
        last_synced_at  TEXT NOT NULL
    );
    INSERT INTO inventory_cards_merged
        SELECT
            cardmarket_id,
            SUM(quantity),
            MIN(name),
            MIN(set_name),
            MIN(set_code),
            MIN(cn),
            condition,
            language,
            is_foil,
            MIN(is_playset),
            is_signed,
            MIN(price),
            MIN(comment),
            MIN(NULLIF(location, '')),
            MIN(name_de),
            MIN(name_es),
            MIN(name_fr),
            MIN(name_it),
            MIN(rarity),
            MIN(listed_at),
            MIN(first_synced_at),
            MAX(last_synced_at)
        FROM inventory_cards
        GROUP BY cardmarket_id, condition, language, is_foil, is_signed;
    DROP TABLE inventory_cards;
    ALTER TABLE inventory_cards_merged RENAME TO inventory_cards;
    CREATE UNIQUE INDEX idx_inventory_article_key
        ON inventory_cards (cardmarket_id, condition, language, is_foil, is_signed);
    COMMIT;
";

/// Creates or migrates the `inventory_cards` table to the current schema (v2).
///
/// Migration chain:
/// - Fresh DB  → create v2 schema directly.
/// - v1 (cardmarket_id PRIMARY KEY, no composite index) → v2.
/// - v2 (composite 5-field key, `idx_inventory_article_key`) → no-op.
/// - v3 (composite 6-field key with location, `idx_inventory_article_key_v3`) → v2
///   (collapses per-location rows, sums quantities).
fn init_schema(conn: &Connection) -> DbResult<()> {
    let table_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='inventory_cards'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();

    if !table_exists {
        return conn.execute_batch(INVENTORY_CARDS_DDL);
    }

    // v3 is identified by idx_inventory_article_key_v3 (6-field key including location).
    // Downgrade to v2 by aggregating per-location rows.
    let v3_index_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master \
             WHERE type='index' AND name='idx_inventory_article_key_v3'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();

    if v3_index_exists {
        log::info!("Migrating inventory_db: collapsing location rows into variants (v3 → v2)");
        return conn.execute_batch(MIGRATION_V3_TO_V2);
    }

    // v2 is identified by idx_inventory_article_key (5-field key, no location).
    let v2_index_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master \
             WHERE type='index' AND name='idx_inventory_article_key'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();

    if !v2_index_exists {
        // v1 schema: single cardmarket_id PRIMARY KEY. Migrate to v2.
        log::info!("Migrating inventory_db: adding composite article key (v1 → v2)");
        conn.execute_batch(MIGRATION_V1_TO_V2)?;
    }

    Ok(())
}

/// Builds a stable composite key string identifying a unique card variant.
///
/// Used during sync to detect which DB rows are no longer present in the CSV.
/// ASCII unit separator (0x1F) is used as delimiter — it cannot appear in field values.
fn article_key(
    id: &str,
    condition: &str,
    language: &str,
    is_foil: &str,
    is_signed: &str,
) -> String {
    format!("{id}\x1F{condition}\x1F{language}\x1F{is_foil}\x1F{is_signed}")
}

/// Returns today's date as `YYYY-MM-DD` using local system time.
fn today_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Syncs a slice of cards (from a freshly loaded inventory CSV) to the local DB.
///
/// - Cards are pre-aggregated by variant (cardmarket_id + condition + language + foil +
///   signed): quantities across multiple locations are summed into a single DB row.
/// - Existing variants are updated; new variants are inserted.
/// - Variants not in `cards` that have `quantity > 0` are zeroed out.
/// - Timestamps are only advanced once per day.
///
/// Errors are returned to the caller; call sites treat them as non-fatal.
pub fn sync_inventory(cards: &[Card]) -> DbResult<SyncStats> {
    let mut conn = open_db()?;
    sync_inventory_conn(&mut conn, cards, &today_date())
}

/// Queries aggregate statistics from the local inventory database.
pub fn get_db_stats() -> DbResult<DbStats> {
    let conn = open_db()?;
    get_db_stats_conn(&conn)
}

/// Inner stats query that accepts an explicit connection — used in tests.
fn get_db_stats_conn(conn: &Connection) -> DbResult<DbStats> {
    let (total_articles, in_stock_articles, total_copies, total_value): (i64, i64, i64, f64) = conn
        .query_row(
            "SELECT
                COUNT(*),
                COALESCE(SUM(CASE WHEN quantity > 0 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(quantity), 0),
                COALESCE(SUM(CAST(price AS REAL) * quantity), 0.0)
             FROM inventory_cards",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;

    let foil_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM inventory_cards
         WHERE quantity > 0 AND (is_foil = '1' OR LOWER(is_foil) = 'true')",
        [],
        |r| r.get(0),
    )?;

    let signed_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM inventory_cards
         WHERE quantity > 0 AND (is_signed = '1' OR LOWER(is_signed) = 'true')",
        [],
        |r| r.get(0),
    )?;

    let top_by_quantity: Vec<(String, i64)> = conn
        .prepare(
            "SELECT name, SUM(quantity) AS total FROM inventory_cards
             WHERE quantity > 0 GROUP BY name ORDER BY total DESC LIMIT 5",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<DbResult<Vec<_>>>()?;

    let top_by_price: Vec<(String, f64)> = conn
        .prepare(
            "SELECT name, CAST(price AS REAL) AS p FROM inventory_cards
             WHERE quantity > 0 AND CAST(price AS REAL) > 0
             ORDER BY p DESC LIMIT 5",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<DbResult<Vec<_>>>()?;

    // Use listed_at when available; fall back to first_synced_at when empty.
    let top_oldest_in_stock: Vec<OldestInStockEntry> = conn
        .prepare(
            "SELECT name,
                    COALESCE(NULLIF(listed_at, ''), first_synced_at) AS effective_date,
                    CAST(price AS REAL),
                    quantity,
                    COALESCE(location, '')
             FROM inventory_cards
             WHERE quantity > 0
             ORDER BY effective_date ASC, quantity DESC LIMIT 5",
        )?
        .query_map([], |r| {
            Ok(OldestInStockEntry {
                name: r.get(0)?,
                date: r.get(1)?,
                price: r.get(2)?,
                quantity: r.get(3)?,
                location: r.get(4)?,
            })
        })?
        .collect::<DbResult<Vec<_>>>()?;

    let oldest_listed: Option<(String, String)> = conn
        .query_row(
            "SELECT name, COALESCE(NULLIF(listed_at, ''), first_synced_at)
             FROM inventory_cards WHERE quantity > 0
             ORDER BY COALESCE(NULLIF(listed_at, ''), first_synced_at) ASC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;

    let newest_listed: Option<(String, String)> = conn
        .query_row(
            "SELECT name, COALESCE(NULLIF(listed_at, ''), first_synced_at)
             FROM inventory_cards WHERE quantity > 0
             ORDER BY COALESCE(NULLIF(listed_at, ''), first_synced_at) DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;

    // MIN on an empty table returns a single row with NULL, so we use Option<String>
    let first_synced_date: Option<String> = conn.query_row(
        "SELECT MIN(first_synced_at) FROM inventory_cards",
        [],
        |r| r.get(0),
    )?;

    let language_breakdown: Vec<(String, i64)> = conn
        .prepare(
            "SELECT language, SUM(quantity) AS total FROM inventory_cards
             WHERE quantity > 0 GROUP BY language ORDER BY total DESC",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<DbResult<Vec<_>>>()?;

    let condition_breakdown: Vec<(String, i64)> = conn
        .prepare(
            "SELECT condition, SUM(quantity) AS total FROM inventory_cards
             WHERE quantity > 0 GROUP BY condition ORDER BY total DESC",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<DbResult<Vec<_>>>()?;

    let rarity_breakdown: Vec<(String, i64)> = conn
        .prepare(
            "SELECT rarity, SUM(quantity) AS total FROM inventory_cards
             WHERE quantity > 0 GROUP BY rarity ORDER BY total DESC",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<DbResult<Vec<_>>>()?;

    Ok(DbStats {
        total_articles,
        in_stock_articles,
        total_copies,
        total_value,
        foil_count,
        signed_count,
        top_by_quantity,
        top_by_price,
        top_oldest_in_stock,
        oldest_listed,
        newest_listed,
        first_synced_date,
        language_breakdown,
        condition_breakdown,
        rarity_breakdown,
    })
}

/// Inner sync that accepts an explicit connection and date — used in tests.
fn sync_inventory_conn(conn: &mut Connection, cards: &[Card], today: &str) -> DbResult<SyncStats> {
    log::debug!("Syncing {} cards to inventory DB ({})", cards.len(), today);
    let tx = conn.transaction()?;
    let mut stats = SyncStats::default();

    // Pre-aggregate cards by (cardmarket_id, condition, language, is_foil, is_signed),
    // summing quantities. The same card variant can appear in multiple CSV rows when it is
    // stored across different physical locations; the DB keeps one row per variant.
    #[allow(clippy::type_complexity)]
    let mut agg: std::collections::HashMap<(&str, &str, &str, &str, &str), (&Card, i64)> =
        std::collections::HashMap::new();
    for card in cards {
        let key = (
            card.cardmarket_id.as_str(),
            card.condition.as_str(),
            card.language.as_str(),
            card.is_foil.as_str(),
            card.is_signed.as_str(),
        );
        let qty: i64 = card.quantity.trim().parse().unwrap_or(0);
        let entry = agg.entry(key).or_insert((card, 0));
        entry.1 += qty;
    }

    // Phase 1: upsert all aggregated card variants
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO inventory_cards (
                cardmarket_id, quantity, name, set_name, set_code, cn,
                condition, language, is_foil, is_playset, is_signed,
                price, comment, location, name_de, name_es, name_fr, name_it,
                rarity, listed_at, first_synced_at, last_synced_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9, ?10, ?11,
                ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                ?19, ?20, ?21, ?21
            )
            ON CONFLICT(cardmarket_id, condition, language, is_foil, is_signed) DO UPDATE SET
                quantity        = excluded.quantity,
                name            = excluded.name,
                set_name        = excluded.set_name,
                set_code        = excluded.set_code,
                cn              = excluded.cn,
                is_playset      = excluded.is_playset,
                price           = excluded.price,
                comment         = excluded.comment,
                location        = excluded.location,
                name_de         = excluded.name_de,
                name_es         = excluded.name_es,
                name_fr         = excluded.name_fr,
                name_it         = excluded.name_it,
                rarity          = excluded.rarity,
                listed_at       = excluded.listed_at,
                last_synced_at  = CASE
                    WHEN last_synced_at = excluded.last_synced_at THEN last_synced_at
                    ELSE excluded.last_synced_at
                END
                -- condition/language/is_foil/is_signed are the unique key; they don't change.
                -- first_synced_at is intentionally excluded: preserved from the original INSERT.",
        )?;

        for (rep_card, total_qty) in agg.values() {
            stmt.execute(params![
                rep_card.cardmarket_id,
                total_qty,
                rep_card.name,
                rep_card.set,
                rep_card.set_code,
                rep_card.cn,
                rep_card.condition,
                rep_card.language,
                rep_card.is_foil,
                rep_card.is_playset,
                rep_card.is_signed,
                rep_card.price,
                rep_card.comment,
                rep_card.location,
                rep_card.name_de,
                rep_card.name_es,
                rep_card.name_fr,
                rep_card.name_it,
                rep_card.rarity,
                rep_card.listed_at,
                today,
            ])?;
            stats.upserted += 1;
        }
    }

    // Phase 2: zero out card variants no longer in the CSV (identified by composite key).
    let csv_keys: HashSet<String> = cards
        .iter()
        .map(|c| {
            article_key(
                &c.cardmarket_id,
                &c.condition,
                &c.language,
                &c.is_foil,
                &c.is_signed,
            )
        })
        .collect();

    // Collect all article keys, quantities, and timestamps from DB.
    let db_rows: Vec<(String, String, String, String, String, i64, String)> = tx
        .prepare(
            "SELECT cardmarket_id, condition, language, is_foil, is_signed,
                    quantity, last_synced_at FROM inventory_cards",
        )?
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })?
        .collect::<DbResult<Vec<_>>>()?;

    for (id, condition, language, is_foil, is_signed, quantity, last_synced_at) in &db_rows {
        let key = article_key(id, condition, language, is_foil, is_signed);
        if !csv_keys.contains(&key) && *quantity > 0 {
            let new_date = if last_synced_at == today {
                last_synced_at.as_str()
            } else {
                today
            };
            tx.execute(
                "UPDATE inventory_cards
                 SET quantity = 0, last_synced_at = ?1
                 WHERE cardmarket_id = ?2 AND condition = ?3 AND language = ?4
                   AND is_foil = ?5 AND is_signed = ?6",
                params![new_date, id, condition, language, is_foil, is_signed],
            )?;
            stats.zeroed += 1;
        }
    }

    tx.commit()?;
    if stats.upserted > 0 || stats.zeroed > 0 {
        log::info!(
            "Inventory DB sync: {} variants updated, {} zeroed",
            stats.upserted,
            stats.zeroed
        );
    } else {
        log::debug!("Inventory DB sync: no changes (same-day re-sync)");
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        // Enable WAL for consistency with production behaviour
        conn.execute_batch("PRAGMA journal_mode=WAL;").ok();
        conn
    }

    fn make_card(id: &str, name: &str, qty: &str) -> Card {
        Card {
            cardmarket_id: id.to_string(),
            quantity: qty.to_string(),
            name: name.to_string(),
            set: "Test Set".to_string(),
            set_code: "TST".to_string(),
            cn: "1".to_string(),
            condition: "NM".to_string(),
            language: "English".to_string(),
            is_foil: "".to_string(),
            is_playset: None,
            is_signed: "".to_string(),
            price: "1.00".to_string(),
            comment: "".to_string(),
            location: None,
            name_de: "".to_string(),
            name_es: "".to_string(),
            name_fr: "".to_string(),
            name_it: "".to_string(),
            rarity: "Common".to_string(),
            listed_at: "2026-01-01".to_string(),
        }
    }

    fn count_rows(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM inventory_cards", [], |r| r.get(0))
            .unwrap()
    }

    fn get_row(conn: &Connection, id: &str) -> Option<(i64, String, String)> {
        conn.query_row(
            "SELECT quantity, first_synced_at, last_synced_at FROM inventory_cards WHERE cardmarket_id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok()
    }

    #[test]
    fn schema_creates_table() {
        let conn = test_conn();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='inventory_cards'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn sync_inserts_new_cards() {
        let mut conn = test_conn();
        let cards = vec![
            make_card("100", "Lightning Bolt", "4"),
            make_card("200", "Counterspell", "2"),
        ];
        let stats = sync_inventory_conn(&mut conn, &cards, "2026-01-01").unwrap();
        assert_eq!(stats.upserted, 2);
        assert_eq!(count_rows(&conn), 2);
    }

    #[test]
    fn sync_sets_first_synced_at_on_insert() {
        let mut conn = test_conn();
        let cards = vec![make_card("100", "Black Lotus", "1")];
        sync_inventory_conn(&mut conn, &cards, "2026-01-15").unwrap();

        let (_, first, last) = get_row(&conn, "100").unwrap();
        assert_eq!(first, "2026-01-15");
        assert_eq!(last, "2026-01-15");
    }

    #[test]
    fn sync_updates_existing_card_fields() {
        let mut conn = test_conn();
        let cards_v1 = vec![make_card("100", "Old Name", "3")];
        sync_inventory_conn(&mut conn, &cards_v1, "2026-01-01").unwrap();

        let mut card_v2 = make_card("100", "New Name", "5");
        card_v2.price = "9.99".to_string();
        sync_inventory_conn(&mut conn, &[card_v2], "2026-01-02").unwrap();

        let (qty, _, _) = get_row(&conn, "100").unwrap();
        assert_eq!(qty, 5);

        let name: String = conn
            .query_row(
                "SELECT name FROM inventory_cards WHERE cardmarket_id = '100'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(name, "New Name");
    }

    #[test]
    fn sync_preserves_first_synced_at_on_update() {
        let mut conn = test_conn();
        let cards = vec![make_card("100", "Mox Pearl", "1")];
        sync_inventory_conn(&mut conn, &cards, "2026-01-01").unwrap();
        sync_inventory_conn(&mut conn, &cards, "2026-01-02").unwrap();
        sync_inventory_conn(&mut conn, &cards, "2026-01-03").unwrap();

        let (_, first, _) = get_row(&conn, "100").unwrap();
        assert_eq!(first, "2026-01-01", "first_synced_at must never change");
    }

    #[test]
    fn sync_updates_last_synced_at_on_new_day() {
        let mut conn = test_conn();
        let cards = vec![make_card("100", "Mox Ruby", "1")];
        sync_inventory_conn(&mut conn, &cards, "2026-01-01").unwrap();
        sync_inventory_conn(&mut conn, &cards, "2026-01-02").unwrap();

        let (_, _, last) = get_row(&conn, "100").unwrap();
        assert_eq!(last, "2026-01-02");
    }

    #[test]
    fn sync_no_duplicate_timestamp_same_day() {
        let mut conn = test_conn();
        let cards = vec![make_card("100", "Ancestral Recall", "1")];
        sync_inventory_conn(&mut conn, &cards, "2026-02-01").unwrap();
        // Sync again on the same day
        sync_inventory_conn(&mut conn, &cards, "2026-02-01").unwrap();

        let (_, first, last) = get_row(&conn, "100").unwrap();
        // Nothing should change; timestamps remain as the first sync of that day
        assert_eq!(first, "2026-02-01");
        assert_eq!(last, "2026-02-01");
    }

    #[test]
    fn sync_zeros_removed_articles() {
        let mut conn = test_conn();
        let day1 = vec![
            make_card("100", "Black Lotus", "1"),
            make_card("200", "Mox Pearl", "2"),
        ];
        sync_inventory_conn(&mut conn, &day1, "2026-01-01").unwrap();

        // Day 2: only card 100 remains in CSV
        let day2 = vec![make_card("100", "Black Lotus", "1")];
        let stats = sync_inventory_conn(&mut conn, &day2, "2026-01-02").unwrap();

        assert_eq!(stats.zeroed, 1);
        let (qty, _, _) = get_row(&conn, "200").unwrap();
        assert_eq!(qty, 0, "Removed card should have quantity 0");
        assert_eq!(count_rows(&conn), 2, "Row should not be deleted");
    }

    #[test]
    fn sync_updates_last_synced_on_zero() {
        let mut conn = test_conn();
        let day1 = vec![make_card("100", "Time Walk", "1")];
        sync_inventory_conn(&mut conn, &day1, "2026-01-01").unwrap();

        // Card removed from CSV on day 2
        sync_inventory_conn(&mut conn, &[], "2026-01-02").unwrap();

        let (qty, _, last) = get_row(&conn, "100").unwrap();
        assert_eq!(qty, 0);
        assert_eq!(last, "2026-01-02");
    }

    #[test]
    fn sync_no_timestamp_when_already_zero() {
        let mut conn = test_conn();
        let day1 = vec![make_card("100", "Timetwister", "1")];
        sync_inventory_conn(&mut conn, &day1, "2026-01-01").unwrap();

        // Card removed on day 2, gets zeroed
        sync_inventory_conn(&mut conn, &[], "2026-01-02").unwrap();
        let (_, _, last_after_zero) = get_row(&conn, "100").unwrap();
        assert_eq!(last_after_zero, "2026-01-02");

        // Day 3: still not in CSV, already at qty 0 → last_synced_at must NOT advance
        sync_inventory_conn(&mut conn, &[], "2026-01-03").unwrap();
        let (qty, _, last_day3) = get_row(&conn, "100").unwrap();
        assert_eq!(qty, 0);
        assert_eq!(
            last_day3, "2026-01-02",
            "Already-zero card must not get new timestamp"
        );
    }

    #[test]
    fn sync_stats_correct() {
        let mut conn = test_conn();
        let initial = vec![
            make_card("1", "A", "1"),
            make_card("2", "B", "1"),
            make_card("3", "C", "1"),
        ];
        sync_inventory_conn(&mut conn, &initial, "2026-01-01").unwrap();

        // Next day: 1 and 2 remain, 3 is removed, 4 is new
        let next = vec![
            make_card("1", "A", "1"),
            make_card("2", "B", "1"),
            make_card("4", "D", "1"),
        ];
        let stats = sync_inventory_conn(&mut conn, &next, "2026-01-02").unwrap();

        assert_eq!(stats.upserted, 3); // 1, 2, and 4
        assert_eq!(stats.zeroed, 1); // 3
    }

    #[test]
    fn today_date_format() {
        let date = today_date();
        assert_eq!(date.len(), 10);
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[7..8], "-");
        // Parseable as a date
        assert!(chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_ok());
    }

    #[test]
    fn get_db_stats_empty_db() {
        let conn = test_conn();
        let stats = get_db_stats_conn(&conn).unwrap();
        assert_eq!(stats.total_articles, 0);
        assert_eq!(stats.in_stock_articles, 0);
        assert_eq!(stats.total_copies, 0);
        assert!((stats.total_value - 0.0).abs() < 0.001);
        assert_eq!(stats.foil_count, 0);
        assert_eq!(stats.signed_count, 0);
        assert!(stats.top_by_quantity.is_empty());
        assert!(stats.top_by_price.is_empty());
        assert!(
            stats.top_oldest_in_stock.is_empty(),
            "top_oldest_in_stock must be empty for empty db"
        );
        assert!(stats.oldest_listed.is_none());
        assert!(stats.newest_listed.is_none());
        assert!(stats.first_synced_date.is_none());
        assert!(stats.language_breakdown.is_empty());
        assert!(stats.condition_breakdown.is_empty());
        assert!(stats.rarity_breakdown.is_empty());
    }

    #[test]
    fn get_db_stats_counts_correctly() {
        let mut conn = test_conn();
        let mut bolt = make_card("1", "Lightning Bolt", "4");
        bolt.price = "2.00".to_string();
        bolt.is_foil = "1".to_string();
        bolt.listed_at = "2024-01-01".to_string();
        bolt.language = "English".to_string();
        bolt.condition = "NM".to_string();
        bolt.rarity = "Common".to_string();

        let mut lotus = make_card("2", "Black Lotus", "1");
        lotus.price = "1000.00".to_string();
        lotus.listed_at = "2024-06-01".to_string();
        lotus.language = "English".to_string();
        lotus.condition = "EX".to_string();
        lotus.rarity = "Rare".to_string();

        sync_inventory_conn(&mut conn, &[bolt, lotus], "2026-01-01").unwrap();
        let stats = get_db_stats_conn(&conn).unwrap();

        assert_eq!(stats.total_articles, 2);
        assert_eq!(stats.in_stock_articles, 2);
        assert_eq!(stats.total_copies, 5); // 4 + 1
        assert!((stats.total_value - (2.00 * 4.0 + 1000.00)).abs() < 0.01);
        assert_eq!(stats.foil_count, 1);
        assert_eq!(stats.top_by_quantity[0].0, "Lightning Bolt");
        assert_eq!(stats.top_by_quantity[0].1, 4);
        assert_eq!(stats.top_by_price[0].0, "Black Lotus");
        assert!((stats.top_by_price[0].1 - 1000.0).abs() < 0.01);
        assert_eq!(stats.top_oldest_in_stock[0].name, "Lightning Bolt");
        assert_eq!(stats.top_oldest_in_stock[0].date, "2024-01-01");
        assert_eq!(stats.top_oldest_in_stock[0].quantity, 4);
        assert_eq!(stats.oldest_listed.as_ref().unwrap().1, "2024-01-01");
        assert_eq!(stats.newest_listed.as_ref().unwrap().1, "2024-06-01");
        assert_eq!(stats.first_synced_date.as_deref(), Some("2026-01-01"));
        assert_eq!(stats.language_breakdown[0].0, "English");
        assert_eq!(stats.language_breakdown[0].1, 5);
        assert_eq!(stats.condition_breakdown.len(), 2);
        assert_eq!(stats.rarity_breakdown.len(), 2);
    }

    #[test]
    fn different_language_variants_stored_as_separate_rows() {
        let mut conn = test_conn();

        let mut en = make_card("571299", "+2 Mace", "1");
        en.language = "English".to_string();
        let mut de = make_card("571299", "+2 Mace", "3");
        de.language = "German".to_string();

        sync_inventory_conn(&mut conn, &[en, de], "2026-01-01").unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM inventory_cards WHERE cardmarket_id = '571299'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 2,
            "English and German variants must be separate rows"
        );

        let total_qty: i64 = conn
            .query_row(
                "SELECT SUM(quantity) FROM inventory_cards WHERE cardmarket_id = '571299'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total_qty, 4); // 1 + 3
    }

    #[test]
    fn same_card_different_locations_quantities_are_summed() {
        let mut conn = test_conn();

        // Same card, same condition/language/foil/signed, two physical locations
        let mut card_a = make_card("750892", "Shock", "1");
        card_a.condition = "EX".to_string();
        card_a.language = "German".to_string();
        card_a.location = Some("A-0-3-30-L12-R".to_string());

        let mut card_b = make_card("750892", "Shock", "1");
        card_b.condition = "EX".to_string();
        card_b.language = "German".to_string();
        card_b.location = Some("B-0-1-57-L4-R".to_string());

        sync_inventory_conn(&mut conn, &[card_a, card_b], "2026-01-01").unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM inventory_cards WHERE cardmarket_id = '750892'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "Same card variant must be stored as one row");

        let qty: i64 = conn
            .query_row(
                "SELECT quantity FROM inventory_cards WHERE cardmarket_id = '750892'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(qty, 2, "Quantities from both locations must be summed");
    }

    #[test]
    fn migration_v1_to_v2_preserves_data_and_adds_index() {
        // Build old schema manually (cardmarket_id PRIMARY KEY = v1).
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE inventory_cards (
                cardmarket_id TEXT NOT NULL PRIMARY KEY,
                quantity INTEGER NOT NULL,
                name TEXT NOT NULL, set_name TEXT NOT NULL, set_code TEXT NOT NULL,
                cn TEXT NOT NULL, condition TEXT NOT NULL, language TEXT NOT NULL,
                is_foil TEXT NOT NULL, is_playset TEXT, is_signed TEXT NOT NULL,
                price TEXT NOT NULL, comment TEXT NOT NULL, location TEXT,
                name_de TEXT NOT NULL, name_es TEXT NOT NULL, name_fr TEXT NOT NULL,
                name_it TEXT NOT NULL, rarity TEXT NOT NULL, listed_at TEXT NOT NULL,
                first_synced_at TEXT NOT NULL, last_synced_at TEXT NOT NULL
            );
            INSERT INTO inventory_cards VALUES
                ('1', 4, 'Lightning Bolt', 'Alpha', 'LEA', '1', 'NM', 'English',
                 '', NULL, '', '2.00', '', NULL, '', '', '', '', 'Common',
                 '2024-01-01', '2026-01-01', '2026-01-01'),
                ('2', 2, 'Counterspell', 'Alpha', 'LEA', '2', 'EX', 'German',
                 '', NULL, '', '5.00', '', NULL, '', '', '', '', 'Common',
                 '2024-01-01', '2026-01-01', '2026-01-01');",
        )
        .unwrap();

        init_schema(&conn).unwrap();

        let index_exists: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master \
                 WHERE type='index' AND name='idx_inventory_article_key'",
                [],
                |_| Ok(true),
            )
            .optional()
            .unwrap()
            .unwrap_or(false);
        assert!(index_exists, "Composite index must exist after migration");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM inventory_cards", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2, "All rows must be preserved after migration");

        // Second call must be a no-op (idempotent).
        init_schema(&conn).unwrap();
        let count2: i64 = conn
            .query_row("SELECT COUNT(*) FROM inventory_cards", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count2, 2);
    }

    #[test]
    fn migration_v3_to_v2_aggregates_quantities() {
        // Build v3 schema manually (6-field key including location).
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE inventory_cards (
                cardmarket_id TEXT NOT NULL, quantity INTEGER NOT NULL,
                name TEXT NOT NULL, set_name TEXT NOT NULL, set_code TEXT NOT NULL,
                cn TEXT NOT NULL, condition TEXT NOT NULL, language TEXT NOT NULL,
                is_foil TEXT NOT NULL, is_playset TEXT, is_signed TEXT NOT NULL,
                price TEXT NOT NULL, comment TEXT NOT NULL,
                location TEXT NOT NULL DEFAULT '',
                name_de TEXT NOT NULL, name_es TEXT NOT NULL, name_fr TEXT NOT NULL,
                name_it TEXT NOT NULL, rarity TEXT NOT NULL, listed_at TEXT NOT NULL,
                first_synced_at TEXT NOT NULL, last_synced_at TEXT NOT NULL
            );
            CREATE UNIQUE INDEX idx_inventory_article_key_v3
                ON inventory_cards (cardmarket_id, condition, language, is_foil, is_signed, location);
            -- Same card, same variant, two locations (qty 1 each)
            INSERT INTO inventory_cards VALUES
                ('750892', 1, 'Shock', 'Innistrad', 'ISD', '42', 'EX', 'German',
                 '', NULL, '', '0.12', '', 'A-0-3-30-L12-R', '', '', '', '', 'Common',
                 '2025-01-01', '2026-01-01', '2026-02-01'),
                ('750892', 1, 'Shock', 'Innistrad', 'ISD', '42', 'EX', 'German',
                 '', NULL, '', '0.12', '', 'B-0-1-57-L4-R', '', '', '', '', 'Common',
                 '2025-01-01', '2026-01-15', '2026-02-10');",
        )
        .unwrap();

        // init_schema detects v3 and downgrades.
        init_schema(&conn).unwrap();

        // v2 index must exist; v3 index must not.
        let v2_idx: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_inventory_article_key'",
                [],
                |_| Ok(true),
            )
            .optional()
            .unwrap()
            .unwrap_or(false);
        assert!(v2_idx, "v2 index must exist after v3→v2 migration");

        let v3_idx: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_inventory_article_key_v3'",
                [],
                |_| Ok(true),
            )
            .optional()
            .unwrap()
            .unwrap_or(false);
        assert!(!v3_idx, "v3 index must not exist after downgrade");

        // Two location rows must have been merged into one with summed quantity.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM inventory_cards", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "Two location rows must merge into one");

        let qty: i64 = conn
            .query_row(
                "SELECT quantity FROM inventory_cards WHERE cardmarket_id = '750892'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(qty, 2, "Quantities from both locations must be summed");

        // first_synced_at = earliest, last_synced_at = latest
        let (first, last): (String, String) = conn
            .query_row(
                "SELECT first_synced_at, last_synced_at FROM inventory_cards",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(first, "2026-01-01");
        assert_eq!(last, "2026-02-10");
    }

    #[test]
    fn get_db_stats_separates_in_stock_from_zeroed() {
        let mut conn = test_conn();
        let day1 = vec![
            make_card("1", "Counterspell", "3"),
            make_card("2", "Dark Ritual", "2"),
        ];
        sync_inventory_conn(&mut conn, &day1, "2026-01-01").unwrap();
        // Card 2 removed from CSV → zeroed
        let day2 = vec![make_card("1", "Counterspell", "3")];
        sync_inventory_conn(&mut conn, &day2, "2026-01-02").unwrap();

        let stats = get_db_stats_conn(&conn).unwrap();
        // Both rows exist in DB, only 1 is in stock
        assert_eq!(stats.total_articles, 2);
        assert_eq!(stats.in_stock_articles, 1);
        assert_eq!(stats.total_copies, 3);
        assert!(stats.top_by_quantity.len() == 1);
        assert!(stats.oldest_listed.is_some());
    }

    #[test]
    fn sync_stores_all_card_fields() {
        let mut conn = test_conn();
        let mut card = make_card("999", "Dual Land", "4");
        card.set = "Unlimited".to_string();
        card.set_code = "2ED".to_string();
        card.cn = "287".to_string();
        card.condition = "EX".to_string();
        card.language = "German".to_string();
        card.is_foil = "1".to_string();
        card.is_playset = Some("0".to_string());
        card.is_signed = "0".to_string();
        card.price = "99.99".to_string();
        card.comment = "LP corners".to_string();
        card.location = Some("A1_S1_R1_C1".to_string());
        card.name_de = "Doppelland".to_string();
        card.rarity = "Rare".to_string();
        card.listed_at = "2026-01-10".to_string();

        sync_inventory_conn(&mut conn, &[card], "2026-01-10").unwrap();

        #[allow(clippy::type_complexity)]
        let row: (
            i64,
            String,
            String,
            String,
            String,
            String,
            String,
            Option<String>,
            String,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT quantity, set_name, set_code, cn, condition, language,
                        is_foil, is_playset, name_de, location
                 FROM inventory_cards WHERE cardmarket_id = '999'",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                        r.get(9)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(row.0, 4);
        assert_eq!(row.1, "Unlimited");
        assert_eq!(row.2, "2ED");
        assert_eq!(row.3, "287");
        assert_eq!(row.4, "EX");
        assert_eq!(row.5, "German");
        assert_eq!(row.6, "1");
        assert_eq!(row.7, Some("0".to_string()));
        assert_eq!(row.8, "Doppelland");
        assert_eq!(row.9, Some("A1_S1_R1_C1".to_string()));
    }
}
