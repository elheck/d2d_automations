//! Local SQLite database for inventory sync.
//!
//! Every time a Cardmarket inventory CSV is loaded, the cards are synced here.
//! - Articles no longer in the CSV have their quantity set to 0 (never deleted).
//! - `first_synced_at`: set once on first insert, never changed.
//! - `last_synced_at`: updated to today on each sync, max once per day.
//! - Articles already at quantity 0 and not in the CSV are left untouched.
//! - Multiple CSV rows for the same card variant (same condition/language/foil/signed)
//!   in different physical locations are merged: quantities are summed, one DB row kept.

use crate::models::{canonical_condition, Card, Language};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::PathBuf;

/// Normalise a boolean flag string to the canonical form used in the DB.
///
/// The legacy Cardmarket export wrote `""` for false and `"1"` for true; the
/// inventory-report CSV uses `"false"`/`"true"`. Both sync to the same variant
/// key by folding them onto the legacy encoding here. Without this, loading a
/// new-format CSV against a DB populated from the legacy format would cause
/// every existing variant to look missing and every CSV row to look new —
/// `sold_quantity` would be incorrectly inflated by the "zeroed" ghost rows.
fn normalize_flag(s: &str) -> String {
    match s.trim().to_lowercase().as_str() {
        "1" | "true" => "1".to_string(),
        _ => String::new(),
    }
}

/// Normalise a language value to canonical capitalised form (`"English"`, …).
/// Unknown values pass through unchanged so we never silently discard data.
fn normalize_language(s: &str) -> String {
    Language::parse(s)
        .map(|l| l.as_str().to_string())
        .unwrap_or_else(|| s.to_string())
}

/// Returns true if `card.location` is set and not just whitespace.
fn card_has_location(card: &Card) -> bool {
    card.location
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

/// Result type for database operations
pub type DbResult<T> = Result<T, rusqlite::Error>;

/// Per-lot revenue and stock breakdown.
#[derive(Debug, Clone)]
pub struct LotBreakdown {
    pub lot: String,
    pub in_stock_listings: i64,
    pub in_stock_copies: i64,
    pub in_stock_value: f64,
    pub sold_copies: i64,
    pub sold_revenue: f64,
    /// Recorded acquisition cost for the whole lot, in EUR. `None` when no cost
    /// has been entered yet, which is distinct from a recorded cost of `0.0`.
    pub cost: Option<f64>,
}

impl LotBreakdown {
    /// Realized margin as a fraction of cost: `(revenue − cost) / cost`.
    ///
    /// Based only on money already earned (unsold stock is excluded). Returns
    /// `None` when no cost is recorded, or when the cost is `0.0` (margin is
    /// undefined — division by zero).
    pub fn realized_margin_fraction(&self) -> Option<f64> {
        match self.cost {
            Some(cost) if cost > 0.0 => Some((self.sold_revenue - cost) / cost),
            _ => None,
        }
    }

    /// Whether the lot has paid for itself: recorded revenue ≥ recorded cost.
    /// `None` when no cost is recorded.
    pub fn is_recouped(&self) -> Option<bool> {
        self.cost.map(|cost| self.sold_revenue >= cost)
    }

    /// Cost still to recoup before the lot breaks even, in EUR (never negative).
    /// `None` when no cost is recorded.
    pub fn cost_to_recoup(&self) -> Option<f64> {
        self.cost.map(|cost| (cost - self.sold_revenue).max(0.0))
    }
}

/// One row in the "longest unsold" top-5 list.
#[derive(Debug, Clone)]
pub struct OldestInStockEntry {
    pub name: String,
    /// Effective date: `listed_at` when available, otherwise `first_synced_at`.
    pub date: String,
    pub price: f64,
    pub quantity: i64,
    pub location: String,
}

/// A single in-stock card variant (quantity > 0) pulled from the database.
///
/// Used by analysis features (mispricing, aging) that need per-card data rather
/// than the aggregate figures in [`DbStats`].
#[derive(Debug, Clone)]
pub struct InStockCard {
    pub cardmarket_id: String,
    pub name: String,
    pub set_code: String,
    pub cn: String,
    pub condition: String,
    pub language: String,
    pub is_foil: bool,
    pub rarity: String,
    pub quantity: i64,
    /// The seller's currently-listed unit price, in EUR.
    pub price: f64,
    pub location: String,
    /// `listed_at` when present, otherwise `first_synced_at`. The best available
    /// proxy for "how long has this been sitting in stock".
    pub effective_date: String,
}

/// One per-variant sale recorded during a sync: `copies` of a variant left stock
/// as a sale on `date`, at the `price` they were listed at when they sold.
///
/// Events are deltas, never totals — the same variant may have many rows (one per
/// sync that saw its quantity drop), and summing `copies` per variant matches the
/// increments applied to `inventory_cards.sold_quantity` since event recording
/// began. Discards (write-offs) never produce events.
#[derive(Debug, Clone, PartialEq)]
pub struct SoldEvent {
    pub date: String,
    pub cardmarket_id: String,
    pub condition: String,
    pub language: String,
    /// Normalised flag: `"1"` for foil, empty otherwise.
    pub is_foil: String,
    /// Normalised flag: `"1"` for signed, empty otherwise.
    pub is_signed: String,
    pub copies: i64,
    /// Listed unit price at the time of sale, in EUR.
    pub price: f64,
}

/// A sold-out variant (quantity 0, `sold_quantity` > 0) — raw input for the
/// restock recommendations report (see [`crate::restock`]).
#[derive(Debug, Clone, PartialEq)]
pub struct RestockCandidate {
    pub cardmarket_id: String,
    pub name: String,
    pub set_code: String,
    pub cn: String,
    pub condition: String,
    pub language: String,
    pub is_foil: bool,
    pub rarity: String,
    /// Total copies sold over the variant's lifetime (`sold_quantity`).
    pub sold_copies: i64,
    /// Σ copies × sale price from `sold_events`; copies sold before event
    /// recording began are valued at the last listed price.
    pub realized_revenue: f64,
    /// The price the variant was last listed at, in EUR.
    pub last_price: f64,
    /// Effective listing date: `listed_at` when available, else `first_synced_at`.
    pub listed_date: String,
    /// Date of the last recorded sale; falls back to `last_synced_at` (the sync
    /// that zeroed the variant) for pre-event-log history.
    pub sold_out_date: String,
}

/// One daily inventory snapshot row. `sold_*_cumulative` are running totals since
/// the first sync, so a period's activity is the difference between two rows.
#[derive(Debug, Clone, PartialEq)]
pub struct InventorySnapshot {
    pub date: String,
    pub in_stock_copies: i64,
    pub in_stock_value: f64,
    pub sold_copies_cumulative: i64,
    pub sold_revenue_cumulative: f64,
}

/// Sales throughput derived from the snapshot history.
#[derive(Debug, Clone, PartialEq)]
pub struct SalesVelocity {
    /// Days spanned between the earliest and latest snapshot.
    pub period_days: i64,
    /// Copies sold across the whole tracked period.
    pub sold_copies: i64,
    /// Revenue earned across the whole tracked period, in EUR.
    pub sold_revenue: f64,
    /// Average copies sold per week over the whole tracked period.
    pub copies_per_week: f64,
    /// Average revenue per week over the whole tracked period, in EUR.
    pub revenue_per_week: f64,
    /// Copies sold in the trailing ~7 days, if a snapshot that old exists.
    pub last7_copies: Option<i64>,
    /// Copies sold in the trailing ~30 days, if a snapshot that old exists.
    pub last30_copies: Option<i64>,
}

/// One age bucket in the dead-stock aging report.
#[derive(Debug, Clone, PartialEq)]
pub struct AgingBucket {
    pub label: &'static str,
    /// Inclusive lower age bound in days.
    pub min_days: i64,
    /// Inclusive upper age bound in days; `None` means open-ended.
    pub max_days: Option<i64>,
    /// Number of distinct in-stock variant rows in this bucket.
    pub listings: i64,
    /// Total copies in this bucket.
    pub copies: i64,
    /// Capital tied up in this bucket (Σ price × quantity), in EUR.
    pub value: f64,
}

/// Aggregate statistics about the current inventory database contents.
#[derive(Debug, Default, Clone)]
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
    /// Revenue and stock breakdown per lot (extracted from location field)
    pub lot_breakdown: Vec<LotBreakdown>,
    /// Dead-stock aging: in-stock cards bucketed by how long they've been listed.
    pub aging_buckets: Vec<AgingBucket>,
    /// Sales velocity derived from snapshot history; `None` until ≥2 days of data.
    pub velocity: Option<SalesVelocity>,
}

/// Statistics from a sync operation
#[derive(Debug, Default)]
pub struct SyncStats {
    /// Number of unique card variants from the CSV that were inserted or updated
    pub upserted: usize,
    /// Number of card variants no longer in the CSV that were set to quantity 0
    pub zeroed: usize,
}

/// Statistics from a discard (write-off) operation.
#[derive(Debug, Default, PartialEq)]
pub struct DiscardStats {
    /// Number of distinct DB variant rows whose quantity was reduced.
    pub variants_updated: usize,
    /// Total copies actually removed from stock (after clamping at the row's
    /// available quantity).
    pub copies_discarded: i64,
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
        last_synced_at  TEXT NOT NULL,
        sold_quantity   INTEGER NOT NULL DEFAULT 0
    );
    CREATE UNIQUE INDEX idx_inventory_article_key
        ON inventory_cards (cardmarket_id, condition, language, is_foil, is_signed);
";

// Daily point-in-time snapshot of the whole inventory, written once per sync day.
// Enables period-over-period sales velocity: because `sold_copies` and
// `sold_revenue` are stored *cumulatively*, the difference between any two
// snapshot dates is exactly what sold (and was earned) in that window.
// `date` is the primary key so re-syncing on the same day overwrites its row.
const INVENTORY_SNAPSHOTS_DDL: &str = "
    CREATE TABLE IF NOT EXISTS inventory_snapshots (
        date                    TEXT PRIMARY KEY,
        in_stock_copies         INTEGER NOT NULL,
        in_stock_value          REAL NOT NULL,
        sold_copies_cumulative  INTEGER NOT NULL,
        sold_revenue_cumulative REAL NOT NULL
    );
";

// Per-variant sale log: one row per sold delta detected during a sync. Whereas
// `inventory_cards.sold_quantity` and the snapshots only carry cumulative totals,
// these rows say *which* variant sold *when* and at what listed price — the basis
// for per-card velocity, restock recommendations and realized-price analysis.
// A same-day re-sync may append several rows for one variant; they are deltas,
// so summing them is always correct.
const SOLD_EVENTS_DDL: &str = "
    CREATE TABLE IF NOT EXISTS sold_events (
        date          TEXT NOT NULL,
        cardmarket_id TEXT NOT NULL,
        condition     TEXT NOT NULL,
        language      TEXT NOT NULL,
        is_foil       TEXT NOT NULL,
        is_signed     TEXT NOT NULL,
        copies        INTEGER NOT NULL,
        price         REAL NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_sold_events_variant
        ON sold_events (cardmarket_id, condition, language, is_foil, is_signed);
    CREATE INDEX IF NOT EXISTS idx_sold_events_date ON sold_events (date);
";

// Manually recorded acquisition cost per lot. One row per lot ID (e.g. `L12`),
// storing the total price paid for that purchase. Orthogonal to the card schema;
// created on every open like the snapshot and sold-event tables. `updated_at`
// records when the figure was last edited so a correction is auditable.
const LOT_COSTS_DDL: &str = "
    CREATE TABLE IF NOT EXISTS lot_costs (
        lot        TEXT PRIMARY KEY,
        cost       REAL NOT NULL,
        updated_at TEXT NOT NULL
    );
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
        conn.execute_batch(INVENTORY_CARDS_DDL)?;
        conn.execute_batch(INVENTORY_SNAPSHOTS_DDL)?;
        conn.execute_batch(SOLD_EVENTS_DDL)?;
        return conn.execute_batch(LOT_COSTS_DDL);
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

    // Add sold_quantity column if missing (added for lot revenue tracking).
    let has_sold_quantity: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('inventory_cards') WHERE name='sold_quantity'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();

    if !has_sold_quantity {
        log::info!("Adding sold_quantity column to inventory_cards");
        conn.execute_batch(
            "ALTER TABLE inventory_cards ADD COLUMN sold_quantity INTEGER NOT NULL DEFAULT 0;",
        )?;
    }

    // Snapshot and sold-event tables are orthogonal to the card-schema migrations;
    // ensure they exist on every open regardless of which card-schema version we
    // came from.
    conn.execute_batch(INVENTORY_SNAPSHOTS_DDL)?;
    conn.execute_batch(SOLD_EVENTS_DDL)?;
    conn.execute_batch(LOT_COSTS_DDL)?;

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

/// Writes off (discards) copies of card variants **without** recording them as
/// sales.
///
/// Each `(card, quantity)` pair reduces the matching DB row's `quantity` by
/// `quantity`, clamped so it never goes below zero. Crucially, `sold_quantity`
/// is left untouched — a discard is stock leaving the shelf that was *not* sold,
/// so it must not inflate tracked revenue. Multiple pairs targeting the same
/// variant (e.g. the same card picked from two locations) are summed first.
///
/// The caller is expected to also export a stock-update CSV of the same copies
/// and import it into Cardmarket, so the next full inventory sync sees the drop
/// already reflected in both places and records no phantom sale.
///
/// Errors are returned to the caller; call sites treat them as non-fatal.
pub fn discard_cards(discards: &[(Card, i64)]) -> DbResult<DiscardStats> {
    let mut conn = open_db()?;
    discard_cards_conn(&mut conn, discards)
}

/// Inner discard that accepts an explicit connection — used in tests.
fn discard_cards_conn(conn: &mut Connection, discards: &[(Card, i64)]) -> DbResult<DiscardStats> {
    // Aggregate requested copies by the same canonical variant key the sync uses,
    // so two rows for the same variant (different physical locations) collapse into
    // a single UPDATE and clamp against the one merged DB row.
    let mut agg: std::collections::HashMap<(String, String, String, String, String), i64> =
        std::collections::HashMap::new();
    for (card, qty) in discards {
        if *qty <= 0 {
            continue;
        }
        let key = (
            card.cardmarket_id.clone(),
            canonical_condition(&card.condition),
            normalize_language(&card.language),
            normalize_flag(&card.is_foil),
            normalize_flag(&card.is_signed),
        );
        *agg.entry(key).or_insert(0) += *qty;
    }

    let tx = conn.transaction()?;
    let mut stats = DiscardStats::default();

    for ((id, cond, lang, foil, signed), requested) in &agg {
        // Read the current quantity so we can clamp and report the true amount
        // removed. sold_quantity is deliberately never referenced here.
        let current: Option<i64> = tx
            .query_row(
                "SELECT quantity FROM inventory_cards
                 WHERE cardmarket_id = ?1 AND condition = ?2 AND language = ?3
                   AND is_foil = ?4 AND is_signed = ?5",
                params![id, cond, lang, foil, signed],
                |r| r.get(0),
            )
            .optional()?;

        let Some(current) = current else {
            log::warn!(
                "Discard skipped: no DB row for variant {id}/{cond}/{lang} (foil={foil}, signed={signed})"
            );
            continue;
        };

        let removed = (*requested).min(current.max(0));
        if removed == 0 {
            continue;
        }

        tx.execute(
            "UPDATE inventory_cards SET quantity = quantity - ?1
             WHERE cardmarket_id = ?2 AND condition = ?3 AND language = ?4
               AND is_foil = ?5 AND is_signed = ?6",
            params![removed, id, cond, lang, foil, signed],
        )?;
        stats.variants_updated += 1;
        stats.copies_discarded += removed;
    }

    tx.commit()?;
    if stats.variants_updated > 0 {
        log::info!(
            "Inventory DB discard: {} copies written off across {} variants (revenue unaffected)",
            stats.copies_discarded,
            stats.variants_updated
        );
    }
    Ok(stats)
}

/// Queries aggregate statistics from the local inventory database.
pub fn get_db_stats() -> DbResult<DbStats> {
    let conn = open_db()?;
    get_db_stats_conn(&conn, &today_date())
}

/// Records (or corrects) the total acquisition cost for a lot, in EUR.
///
/// Upserts by lot ID: entering a cost for a lot that already has one overwrites
/// it, so a mistaken buy price can be fixed. `cost` must be non-negative.
pub fn set_lot_cost(lot: &str, cost: f64) -> DbResult<()> {
    let conn = open_db()?;
    set_lot_cost_conn(&conn, lot, cost, &today_date())
}

/// Inner upsert that accepts an explicit connection and date — used in tests.
fn set_lot_cost_conn(conn: &Connection, lot: &str, cost: f64, today: &str) -> DbResult<()> {
    conn.execute(
        "INSERT INTO lot_costs (lot, cost, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(lot) DO UPDATE SET cost = excluded.cost, updated_at = excluded.updated_at",
        params![lot, cost, today],
    )?;
    Ok(())
}

/// Removes the recorded acquisition cost for a lot, if any.
pub fn delete_lot_cost(lot: &str) -> DbResult<()> {
    let conn = open_db()?;
    delete_lot_cost_conn(&conn, lot)
}

/// Inner delete that accepts an explicit connection — used in tests.
fn delete_lot_cost_conn(conn: &Connection, lot: &str) -> DbResult<()> {
    conn.execute("DELETE FROM lot_costs WHERE lot = ?1", params![lot])?;
    Ok(())
}

/// Returns every in-stock card variant (quantity > 0) from the database.
pub fn get_in_stock_cards() -> DbResult<Vec<InStockCard>> {
    let conn = open_db()?;
    get_in_stock_cards_conn(&conn)
}

/// Inner query that accepts an explicit connection — used in tests.
fn get_in_stock_cards_conn(conn: &Connection) -> DbResult<Vec<InStockCard>> {
    conn.prepare(
        "SELECT cardmarket_id, name, set_code, cn, condition, language,
                (is_foil = '1' OR LOWER(is_foil) = 'true') AS foil,
                rarity, quantity, CAST(price AS REAL),
                COALESCE(location, ''),
                COALESCE(NULLIF(listed_at, ''), first_synced_at) AS effective_date
         FROM inventory_cards
         WHERE quantity > 0",
    )?
    .query_map([], |r| {
        Ok(InStockCard {
            cardmarket_id: r.get(0)?,
            name: r.get(1)?,
            set_code: r.get(2)?,
            cn: r.get(3)?,
            condition: r.get(4)?,
            language: r.get(5)?,
            is_foil: r.get(6)?,
            rarity: r.get(7)?,
            quantity: r.get(8)?,
            price: r.get(9)?,
            location: r.get(10)?,
            effective_date: r.get(11)?,
        })
    })?
    .collect()
}

/// Returns every recorded sold event, oldest first.
pub fn get_sold_events() -> DbResult<Vec<SoldEvent>> {
    let conn = open_db()?;
    get_sold_events_conn(&conn)
}

/// Inner query that accepts an explicit connection — used in tests.
fn get_sold_events_conn(conn: &Connection) -> DbResult<Vec<SoldEvent>> {
    conn.prepare(
        "SELECT date, cardmarket_id, condition, language, is_foil, is_signed, copies, price
         FROM sold_events ORDER BY date ASC, rowid ASC",
    )?
    .query_map([], |r| {
        Ok(SoldEvent {
            date: r.get(0)?,
            cardmarket_id: r.get(1)?,
            condition: r.get(2)?,
            language: r.get(3)?,
            is_foil: r.get(4)?,
            is_signed: r.get(5)?,
            copies: r.get(6)?,
            price: r.get(7)?,
        })
    })?
    .collect()
}

/// Returns every sold-out variant (quantity 0, sold copies > 0) enriched with
/// its sale history — the raw input for the restock recommendations report.
pub fn get_restock_candidates() -> DbResult<Vec<RestockCandidate>> {
    let conn = open_db()?;
    get_restock_candidates_conn(&conn)
}

/// Inner query that accepts an explicit connection — used in tests.
///
/// Realized revenue sums the per-sale prices from `sold_events`; any copies sold
/// before event recording existed (sold_quantity larger than the events' total)
/// are valued at the variant's last listed price. The sold-out date prefers the
/// last event date and falls back to `last_synced_at`, which for a zeroed row is
/// the day the sync zeroed it.
fn get_restock_candidates_conn(conn: &Connection) -> DbResult<Vec<RestockCandidate>> {
    conn.prepare(
        "SELECT c.cardmarket_id, c.name, c.set_code, c.cn, c.condition, c.language,
                (c.is_foil = '1' OR LOWER(c.is_foil) = 'true') AS foil,
                c.rarity, c.sold_quantity,
                CAST(c.price AS REAL) AS last_price,
                COALESCE(NULLIF(c.listed_at, ''), c.first_synced_at) AS listed_date,
                COALESCE(e.last_sale_date, c.last_synced_at) AS sold_out_date,
                COALESCE(e.revenue, 0.0)
                    + MAX(c.sold_quantity - COALESCE(e.copies, 0), 0)
                      * CAST(c.price AS REAL) AS realized_revenue
         FROM inventory_cards c
         LEFT JOIN (
             SELECT cardmarket_id, condition, language, is_foil, is_signed,
                    MAX(date) AS last_sale_date,
                    SUM(copies) AS copies,
                    SUM(copies * price) AS revenue
             FROM sold_events
             GROUP BY cardmarket_id, condition, language, is_foil, is_signed
         ) e ON e.cardmarket_id = c.cardmarket_id
            AND e.condition = c.condition
            AND e.language = c.language
            AND e.is_foil = c.is_foil
            AND e.is_signed = c.is_signed
         WHERE c.quantity = 0 AND c.sold_quantity > 0",
    )?
    .query_map([], |r| {
        Ok(RestockCandidate {
            cardmarket_id: r.get(0)?,
            name: r.get(1)?,
            set_code: r.get(2)?,
            cn: r.get(3)?,
            condition: r.get(4)?,
            language: r.get(5)?,
            is_foil: r.get(6)?,
            rarity: r.get(7)?,
            sold_copies: r.get(8)?,
            last_price: r.get(9)?,
            listed_date: r.get(10)?,
            sold_out_date: r.get(11)?,
            realized_revenue: r.get(12)?,
        })
    })?
    .collect()
}

/// Reads all snapshot rows ordered by date ascending.
fn read_snapshots_conn(conn: &Connection) -> DbResult<Vec<InventorySnapshot>> {
    conn.prepare(
        "SELECT date, in_stock_copies, in_stock_value,
                sold_copies_cumulative, sold_revenue_cumulative
         FROM inventory_snapshots ORDER BY date ASC",
    )?
    .query_map([], |r| {
        Ok(InventorySnapshot {
            date: r.get(0)?,
            in_stock_copies: r.get(1)?,
            in_stock_value: r.get(2)?,
            sold_copies_cumulative: r.get(3)?,
            sold_revenue_cumulative: r.get(4)?,
        })
    })?
    .collect()
}

/// Parses a `YYYY-MM-DD` snapshot date.
fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// Computes sales velocity from an ascending-by-date slice of snapshots.
///
/// Pure and deterministic: all figures are derived from the cumulative sold
/// counters carried by the snapshots, so no wall-clock access is needed.
/// Returns `None` until there are at least two snapshots spanning ≥1 day.
fn compute_velocity(snaps: &[InventorySnapshot]) -> Option<SalesVelocity> {
    let earliest = snaps.first()?;
    let latest = snaps.last()?;
    let start = parse_date(&earliest.date)?;
    let end = parse_date(&latest.date)?;
    let period_days = (end - start).num_days();
    if period_days < 1 {
        return None;
    }

    let sold_copies = (latest.sold_copies_cumulative - earliest.sold_copies_cumulative).max(0);
    let sold_revenue = (latest.sold_revenue_cumulative - earliest.sold_revenue_cumulative).max(0.0);
    let weeks = period_days as f64 / 7.0;
    let copies_per_week = sold_copies as f64 / weeks;
    let revenue_per_week = sold_revenue / weeks;

    // Trailing-window sold counts: find the most recent snapshot on or before the
    // target cutoff and diff its cumulative counter against the latest.
    let window_sold = |days: i64| -> Option<i64> {
        let cutoff = end - chrono::Duration::days(days);
        let base = snaps
            .iter()
            .rev()
            .find(|s| parse_date(&s.date).is_some_and(|d| d <= cutoff))?;
        // Only meaningful if the base snapshot is strictly before the latest one.
        if base.date == latest.date {
            return None;
        }
        Some((latest.sold_copies_cumulative - base.sold_copies_cumulative).max(0))
    };

    Some(SalesVelocity {
        period_days,
        sold_copies,
        sold_revenue,
        copies_per_week,
        revenue_per_week,
        last7_copies: window_sold(7),
        last30_copies: window_sold(30),
    })
}

/// Extracts the lot number (e.g. `L0`, `L12`) from a location string.
///
/// Location format: `A-0-0-31-L0-R` — the lot is the first `-`-separated
/// segment that starts with `L` followed by one or more digits.
fn extract_lot_number(location: &str) -> Option<&str> {
    location.split('-').find(|part| {
        part.len() > 1 && part.starts_with('L') && part[1..].bytes().all(|b| b.is_ascii_digit())
    })
}

/// Loads all recorded per-lot acquisition costs into a `lot → cost` map.
fn lot_costs_map(conn: &Connection) -> DbResult<std::collections::HashMap<String, f64>> {
    conn.prepare("SELECT lot, cost FROM lot_costs")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?
        .collect()
}

/// Builds the per-lot revenue breakdown from all inventory rows that carry a
/// location with a recognisable lot number.
fn lot_breakdown_from(conn: &Connection) -> DbResult<Vec<LotBreakdown>> {
    let mut stmt = conn.prepare(
        "SELECT location, quantity, CAST(price AS REAL), sold_quantity
         FROM inventory_cards
         WHERE location IS NOT NULL AND location != ''",
    )?;

    let mut lots: std::collections::HashMap<String, (i64, i64, f64, i64, f64)> =
        std::collections::HashMap::new();

    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, f64>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;

    for row in rows {
        let (location, qty, price, sold_qty) = row?;
        if let Some(lot) = extract_lot_number(&location) {
            if lot == "L0" {
                continue;
            }
            let entry = lots.entry(lot.to_string()).or_default();
            if qty > 0 {
                entry.0 += 1; // in_stock_listings
                entry.1 += qty; // in_stock_copies
                entry.2 += price * qty as f64; // in_stock_value
            }
            entry.3 += sold_qty; // sold_copies
            entry.4 += price * sold_qty as f64; // sold_revenue
        }
    }

    let costs = lot_costs_map(conn)?;

    let mut result: Vec<LotBreakdown> = lots
        .into_iter()
        .map(
            |(
                lot,
                (in_stock_listings, in_stock_copies, in_stock_value, sold_copies, sold_revenue),
            )| {
                let cost = costs.get(&lot).copied();
                LotBreakdown {
                    lot,
                    in_stock_listings,
                    in_stock_copies,
                    in_stock_value,
                    sold_copies,
                    sold_revenue,
                    cost,
                }
            },
        )
        .collect();

    // Sort by lot number numerically (L0, L1, L2, …)
    result.sort_by(|a, b| {
        let num_a: i64 = a.lot[1..].parse().unwrap_or(i64::MAX);
        let num_b: i64 = b.lot[1..].parse().unwrap_or(i64::MAX);
        num_a.cmp(&num_b)
    });

    Ok(result)
}

/// Inner stats query that accepts an explicit connection and reference date —
/// used in tests. `today` (as `YYYY-MM-DD`) anchors the dead-stock aging report.
fn get_db_stats_conn(conn: &Connection, today: &str) -> DbResult<DbStats> {
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

    let lot_breakdown = lot_breakdown_from(conn)?;

    // Dead-stock aging: bucket in-stock cards by listing age relative to `today`.
    let in_stock = get_in_stock_cards_conn(conn)?;
    let aging_buckets = parse_date(today)
        .map(|d| crate::aging::bucket_cards(&in_stock, d))
        .unwrap_or_default();

    // Sales velocity from snapshot history.
    let velocity = compute_velocity(&read_snapshots_conn(conn)?);

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
        lot_breakdown,
        aging_buckets,
        velocity,
    })
}

/// Inner sync that accepts an explicit connection and date — used in tests.
fn sync_inventory_conn(conn: &mut Connection, cards: &[Card], today: &str) -> DbResult<SyncStats> {
    log::debug!("Syncing {} cards to inventory DB ({})", cards.len(), today);
    let tx = conn.transaction()?;
    let mut stats = SyncStats::default();

    // Pre-aggregate cards by (cardmarket_id, condition, language, is_foil, is_signed),
    // summing quantities. The key components are normalised to the canonical form stored
    // in the DB so the legacy Cardmarket export and the new inventory-report CSV produce
    // identical variant keys (see `normalize_flag` / `normalize_language` /
    // `canonical_condition` for the encodings used).
    #[allow(clippy::type_complexity)]
    let mut agg: std::collections::HashMap<
        (String, String, String, String, String),
        (&Card, i64),
    > = std::collections::HashMap::new();
    for card in cards {
        let key = (
            card.cardmarket_id.clone(),
            canonical_condition(&card.condition),
            normalize_language(&card.language),
            normalize_flag(&card.is_foil),
            normalize_flag(&card.is_signed),
        );
        let qty: i64 = card.quantity.trim().parse().unwrap_or(0);
        let entry = agg.entry(key).or_insert((card, 0));
        entry.1 += qty;

        // The inventory-report CSV emits a placeholder "summary" row (quantity 0,
        // empty location) *before* the real per-location row for a variant. If the
        // summary row is seen first, `or_insert` keeps it as the representative and
        // the real location is lost. Prefer any row with a non-empty location over
        // one without — that's the only information the DB's single `location`
        // column can carry, and an empty one is never useful on the "longest unsold"
        // screen.
        let rep_has_loc = card_has_location(entry.0);
        let new_has_loc = card_has_location(card);
        if new_has_loc && !rep_has_loc {
            entry.0 = card;
        }
    }

    // Pre-sync DB state, read once: used to detect per-variant sold deltas (this
    // sync's quantity drops) and to find rows that vanished from the CSV (phase 2).
    // Tuple layout: key fields, quantity, listed price, last_synced_at.
    #[allow(clippy::type_complexity)]
    let db_rows: Vec<(String, String, String, String, String, i64, f64, String)> = tx
        .prepare(
            "SELECT cardmarket_id, condition, language, is_foil, is_signed,
                    quantity, CAST(price AS REAL), last_synced_at FROM inventory_cards",
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
                row.get(7)?,
            ))
        })?
        .collect::<DbResult<Vec<_>>>()?;

    let db_qty_price: std::collections::HashMap<String, (i64, f64)> = db_rows
        .iter()
        .map(|(id, cond, lang, foil, signed, qty, price, _)| {
            (article_key(id, cond, lang, foil, signed), (*qty, *price))
        })
        .collect();

    // Sold events detected this sync: (key fields..., copies, price at sale).
    let mut sold_events: Vec<(String, String, String, String, String, i64, f64)> = Vec::new();

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
                sold_quantity   = CASE
                    WHEN excluded.quantity < inventory_cards.quantity
                    THEN inventory_cards.sold_quantity
                         + (inventory_cards.quantity - excluded.quantity)
                    ELSE inventory_cards.sold_quantity
                END,
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

        for ((id, cond, lang, foil, signed), (rep_card, total_qty)) in &agg {
            // Mirror the upsert's sold_quantity CASE: a lower CSV quantity than the
            // stored one means the difference sold, at the price it was listed at.
            if let Some((old_qty, old_price)) =
                db_qty_price.get(&article_key(id, cond, lang, foil, signed))
            {
                if total_qty < old_qty {
                    sold_events.push((
                        id.clone(),
                        cond.clone(),
                        lang.clone(),
                        foil.clone(),
                        signed.clone(),
                        old_qty - total_qty,
                        *old_price,
                    ));
                }
            }
            stmt.execute(params![
                id,
                total_qty,
                rep_card.name,
                rep_card.set,
                rep_card.set_code,
                rep_card.cn,
                cond,
                lang,
                foil,
                rep_card.is_playset,
                signed,
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
    // Normalise the same way as the aggregation above so keys line up with DB rows.
    let csv_keys: HashSet<String> = cards
        .iter()
        .map(|c| {
            article_key(
                &c.cardmarket_id,
                &canonical_condition(&c.condition),
                &normalize_language(&c.language),
                &normalize_flag(&c.is_foil),
                &normalize_flag(&c.is_signed),
            )
        })
        .collect();

    // Phase 1 only touched CSV variants, so the pre-sync rows read above are still
    // accurate for everything the CSV no longer contains.
    for (id, condition, language, is_foil, is_signed, quantity, price, last_synced_at) in &db_rows {
        let key = article_key(id, condition, language, is_foil, is_signed);
        if !csv_keys.contains(&key) && *quantity > 0 {
            let new_date = if last_synced_at == today {
                last_synced_at.as_str()
            } else {
                today
            };
            tx.execute(
                "UPDATE inventory_cards
                 SET sold_quantity = sold_quantity + quantity,
                     quantity = 0,
                     last_synced_at = ?1
                 WHERE cardmarket_id = ?2 AND condition = ?3 AND language = ?4
                   AND is_foil = ?5 AND is_signed = ?6",
                params![new_date, id, condition, language, is_foil, is_signed],
            )?;
            sold_events.push((
                id.clone(),
                condition.clone(),
                language.clone(),
                is_foil.clone(),
                is_signed.clone(),
                *quantity,
                *price,
            ));
            stats.zeroed += 1;
        }
    }

    // Phase 2b: persist the sold deltas detected above. Rows are appended, never
    // updated — a same-day re-sync that sells more copies simply adds another
    // delta row for the same variant/date.
    if !sold_events.is_empty() {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO sold_events
                (date, cardmarket_id, condition, language, is_foil, is_signed, copies, price)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for (id, cond, lang, foil, signed, copies, price) in &sold_events {
            stmt.execute(params![today, id, cond, lang, foil, signed, copies, price])?;
        }
        log::info!(
            "Inventory DB sync: recorded {} sold event(s)",
            sold_events.len()
        );
    }

    // Phase 3: record today's snapshot from the now-current table state. Cumulative
    // sold figures let later reads diff any two dates into a period velocity.
    // INSERT OR REPLACE keyed on `date` keeps at most one row per day (same-day
    // re-syncs overwrite it with the latest numbers).
    tx.execute(
        "INSERT OR REPLACE INTO inventory_snapshots
            (date, in_stock_copies, in_stock_value,
             sold_copies_cumulative, sold_revenue_cumulative)
         SELECT
            ?1,
            COALESCE(SUM(CASE WHEN quantity > 0 THEN quantity ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN quantity > 0 THEN CAST(price AS REAL) * quantity ELSE 0 END), 0.0),
            COALESCE(SUM(sold_quantity), 0),
            COALESCE(SUM(CAST(price AS REAL) * sold_quantity), 0.0)
         FROM inventory_cards",
        params![today],
    )?;

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
            is_first_ed: None,
            is_reverse_holo: None,
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
        let stats = get_db_stats_conn(&conn, "2026-07-14").unwrap();
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
        assert!(stats.lot_breakdown.is_empty());
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
        let stats = get_db_stats_conn(&conn, "2026-07-14").unwrap();

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
    fn inventory_report_summary_row_does_not_erase_real_location() {
        // Regression: the inventory-report CSV emits a placeholder row with
        // quantity 0 and empty location *before* the real per-location row.
        // Aggregation must not pick the placeholder as the representative,
        // otherwise the Longest-Unsold screen loses the location.
        let mut conn = test_conn();

        let mut summary = make_card("510275", "Brazen Freebooter", "0");
        summary.location = Some(String::new());

        let mut real = make_card("510275", "Brazen Freebooter", "37");
        real.location = Some("A-0-0-25-L0-R".to_string());

        sync_inventory_conn(&mut conn, &[summary, real], "2026-01-01").unwrap();

        let location: Option<String> = conn
            .query_row(
                "SELECT location FROM inventory_cards WHERE cardmarket_id = '510275'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(location.as_deref(), Some("A-0-0-25-L0-R"));

        let qty: i64 = conn
            .query_row(
                "SELECT quantity FROM inventory_cards WHERE cardmarket_id = '510275'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(qty, 37, "summary row quantity must still be summed in");
    }

    #[test]
    fn inventory_report_summary_row_does_not_erase_location_regardless_of_order() {
        // Same scenario but the real row is seen FIRST, then the summary.
        // The summary must not overwrite the already-good representative.
        let mut conn = test_conn();

        let mut real = make_card("510275", "Brazen Freebooter", "37");
        real.location = Some("A-0-0-25-L0-R".to_string());

        let mut summary = make_card("510275", "Brazen Freebooter", "0");
        summary.location = Some(String::new());

        sync_inventory_conn(&mut conn, &[real, summary], "2026-01-01").unwrap();

        let location: Option<String> = conn
            .query_row(
                "SELECT location FROM inventory_cards WHERE cardmarket_id = '510275'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(location.as_deref(), Some("A-0-0-25-L0-R"));
    }

    #[test]
    fn new_format_csv_merges_with_legacy_db_row_no_phantom_sale() {
        // Regression: a legacy-format DB row (condition "NM", language "English",
        // is_foil "", is_signed "") must merge with a new-format inventory-report row
        // (condition "near_mint", language "english", is_foil "false", is_signed "false")
        // — NOT get zeroed out while the new one is inserted alongside.
        let mut conn = test_conn();

        // Seed a legacy-format DB row with 4 copies and no prior sales.
        let legacy = make_card("42", "Bolt", "4");
        assert_eq!(legacy.condition, "NM");
        assert_eq!(legacy.language, "English");
        assert_eq!(legacy.is_foil, "");
        assert_eq!(legacy.is_signed, "");
        sync_inventory_conn(&mut conn, &[legacy], "2026-01-01").unwrap();

        // Sync a new-format card representing the same variant with qty unchanged.
        let mut new_fmt = make_card("42", "Bolt", "4");
        new_fmt.condition = "near_mint".to_string();
        new_fmt.language = "english".to_string();
        new_fmt.is_foil = "false".to_string();
        new_fmt.is_signed = "false".to_string();
        let stats = sync_inventory_conn(&mut conn, &[new_fmt], "2026-01-02").unwrap();

        // Exactly one row, still 4 copies, nothing zeroed, no phantom sale recorded.
        assert_eq!(count_rows(&conn), 1);
        assert_eq!(stats.upserted, 1);
        assert_eq!(stats.zeroed, 0);
        let sold: i64 = conn
            .query_row(
                "SELECT sold_quantity FROM inventory_cards WHERE cardmarket_id = '42'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sold, 0, "no sales occurred — sold_quantity must stay 0");

        // first_synced_at preserved from the original legacy insert.
        let (_, first, _) = get_row(&conn, "42").unwrap();
        assert_eq!(first, "2026-01-01");
    }

    #[test]
    fn new_format_partial_sale_attributed_correctly_against_legacy_row() {
        // Same variant, but the new sync reports only 1 copy remaining → 3 sold.
        let mut conn = test_conn();
        let legacy = make_card("42", "Bolt", "4");
        sync_inventory_conn(&mut conn, &[legacy], "2026-01-01").unwrap();

        let mut new_fmt = make_card("42", "Bolt", "1");
        new_fmt.condition = "near_mint".to_string();
        new_fmt.language = "english".to_string();
        new_fmt.is_foil = "false".to_string();
        new_fmt.is_signed = "false".to_string();
        sync_inventory_conn(&mut conn, &[new_fmt], "2026-01-02").unwrap();

        let (qty, _, _) = get_row(&conn, "42").unwrap();
        assert_eq!(qty, 1);
        let sold: i64 = conn
            .query_row(
                "SELECT sold_quantity FROM inventory_cards WHERE cardmarket_id = '42'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sold, 3);
        assert_eq!(count_rows(&conn), 1, "must not create a duplicate row");
    }

    #[test]
    fn new_format_foil_variant_matches_legacy_one_encoding() {
        // Legacy foil variants are stored as is_foil = "1"; the inventory-report CSV
        // sends "true". Both must hit the same row.
        let mut conn = test_conn();
        let mut legacy_foil = make_card("99", "Shock", "2");
        legacy_foil.is_foil = "1".to_string();
        sync_inventory_conn(&mut conn, &[legacy_foil], "2026-01-01").unwrap();

        let mut new_foil = make_card("99", "Shock", "2");
        new_foil.condition = "near_mint".to_string();
        new_foil.language = "english".to_string();
        new_foil.is_foil = "true".to_string();
        new_foil.is_signed = "false".to_string();
        sync_inventory_conn(&mut conn, &[new_foil], "2026-01-02").unwrap();

        assert_eq!(count_rows(&conn), 1);
        let stored_foil: String = conn
            .query_row(
                "SELECT is_foil FROM inventory_cards WHERE cardmarket_id = '99'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored_foil, "1", "foil flag stored in canonical form");
    }

    #[test]
    fn normalize_flag_folds_representations() {
        assert_eq!(normalize_flag("true"), "1");
        assert_eq!(normalize_flag("TRUE"), "1");
        assert_eq!(normalize_flag("1"), "1");
        assert_eq!(normalize_flag("false"), "");
        assert_eq!(normalize_flag("FALSE"), "");
        assert_eq!(normalize_flag("0"), "");
        assert_eq!(normalize_flag(""), "");
    }

    #[test]
    fn normalize_language_capitalises() {
        assert_eq!(normalize_language("english"), "English");
        assert_eq!(normalize_language("German"), "German");
        assert_eq!(normalize_language("en"), "English");
        // Unknown values pass through to avoid silent data loss.
        assert_eq!(normalize_language("klingon"), "klingon");
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

        let stats = get_db_stats_conn(&conn, "2026-07-14").unwrap();
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

    // ==================== extract_lot_number Tests ====================

    #[test]
    fn extract_lot_from_full_location() {
        assert_eq!(extract_lot_number("A-0-0-31-L0-R"), Some("L0"));
    }

    #[test]
    fn extract_lot_multi_digit() {
        assert_eq!(extract_lot_number("B-0-1-57-L12-R"), Some("L12"));
    }

    #[test]
    fn extract_lot_no_suffix() {
        assert_eq!(extract_lot_number("A-0-1-4-L3"), Some("L3"));
    }

    #[test]
    fn extract_lot_no_lot_in_location() {
        assert_eq!(extract_lot_number("A-0-1-4"), None);
    }

    #[test]
    fn extract_lot_bare_l_not_a_lot() {
        assert_eq!(extract_lot_number("A-0-1-4-L"), None);
    }

    // ==================== sold_quantity Tracking Tests ====================

    #[test]
    fn sold_quantity_tracked_on_full_removal() {
        let mut conn = test_conn();
        let mut card = make_card("1", "Bolt", "3");
        card.location = Some("A-0-0-1-L0-R".to_string());
        card.price = "2.00".to_string();
        sync_inventory_conn(&mut conn, &[card], "2026-01-01").unwrap();

        // Card removed from CSV → all 3 copies sold
        sync_inventory_conn(&mut conn, &[], "2026-01-02").unwrap();

        let sold: i64 = conn
            .query_row(
                "SELECT sold_quantity FROM inventory_cards WHERE cardmarket_id = '1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sold, 3);
    }

    #[test]
    fn sold_quantity_tracked_on_partial_sale() {
        let mut conn = test_conn();
        let mut card = make_card("1", "Bolt", "5");
        card.location = Some("A-0-0-1-L0-R".to_string());
        sync_inventory_conn(&mut conn, &[card], "2026-01-01").unwrap();

        // Qty drops from 5 to 2 → 3 sold
        let mut card2 = make_card("1", "Bolt", "2");
        card2.location = Some("A-0-0-1-L0-R".to_string());
        sync_inventory_conn(&mut conn, &[card2], "2026-01-02").unwrap();

        let sold: i64 = conn
            .query_row(
                "SELECT sold_quantity FROM inventory_cards WHERE cardmarket_id = '1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sold, 3);
    }

    #[test]
    fn sold_quantity_not_incremented_on_restock() {
        let mut conn = test_conn();
        let card = make_card("1", "Bolt", "2");
        sync_inventory_conn(&mut conn, &[card], "2026-01-01").unwrap();

        // Qty increases from 2 to 5 → no sale
        let card2 = make_card("1", "Bolt", "5");
        sync_inventory_conn(&mut conn, &[card2], "2026-01-02").unwrap();

        let sold: i64 = conn
            .query_row(
                "SELECT sold_quantity FROM inventory_cards WHERE cardmarket_id = '1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sold, 0);
    }

    #[test]
    fn sold_quantity_accumulates_across_syncs() {
        let mut conn = test_conn();
        let card = make_card("1", "Bolt", "10");
        sync_inventory_conn(&mut conn, &[card], "2026-01-01").unwrap();

        // Sell 3
        let card2 = make_card("1", "Bolt", "7");
        sync_inventory_conn(&mut conn, &[card2], "2026-01-02").unwrap();

        // Sell 2 more
        let card3 = make_card("1", "Bolt", "5");
        sync_inventory_conn(&mut conn, &[card3], "2026-01-03").unwrap();

        let sold: i64 = conn
            .query_row(
                "SELECT sold_quantity FROM inventory_cards WHERE cardmarket_id = '1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sold, 5);
    }

    // ==================== Discard (write-off) Tests ====================

    fn sold_qty(conn: &Connection, id: &str) -> i64 {
        conn.query_row(
            "SELECT sold_quantity FROM inventory_cards WHERE cardmarket_id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn discard_reduces_quantity_without_touching_sold() {
        let mut conn = test_conn();
        let mut card = make_card("1", "Bolt", "10");
        card.price = "2.00".to_string();
        sync_inventory_conn(&mut conn, &[card.clone()], "2026-01-01").unwrap();

        let stats = discard_cards_conn(&mut conn, &[(card, 3)]).unwrap();
        assert_eq!(stats.variants_updated, 1);
        assert_eq!(stats.copies_discarded, 3);

        let (qty, _, _) = get_row(&conn, "1").unwrap();
        assert_eq!(qty, 7, "quantity reduced by the discarded amount");
        assert_eq!(
            sold_qty(&conn, "1"),
            0,
            "discards must never count as sales"
        );
    }

    #[test]
    fn discard_does_not_inflate_revenue_on_next_sync() {
        // End-to-end: discard, then re-sync a CSV that reflects the reduced stock
        // (as the exported update CSV would after being imported into Cardmarket).
        // No copies must be attributed as sold.
        let mut conn = test_conn();
        let card10 = make_card("1", "Bolt", "10");
        sync_inventory_conn(&mut conn, std::slice::from_ref(&card10), "2026-01-01").unwrap();

        discard_cards_conn(&mut conn, &[(card10, 4)]).unwrap();

        // Cardmarket now reports the post-discard quantity of 6.
        let card6 = make_card("1", "Bolt", "6");
        sync_inventory_conn(&mut conn, &[card6], "2026-01-02").unwrap();

        assert_eq!(
            sold_qty(&conn, "1"),
            0,
            "no phantom sale after discard sync"
        );
        let (qty, _, _) = get_row(&conn, "1").unwrap();
        assert_eq!(qty, 6);
    }

    #[test]
    fn discard_clamps_at_available_quantity() {
        let mut conn = test_conn();
        let card = make_card("1", "Bolt", "2");
        sync_inventory_conn(&mut conn, std::slice::from_ref(&card), "2026-01-01").unwrap();

        // Ask to discard more than exist.
        let stats = discard_cards_conn(&mut conn, &[(card, 5)]).unwrap();
        assert_eq!(stats.copies_discarded, 2, "clamped to available stock");
        let (qty, _, _) = get_row(&conn, "1").unwrap();
        assert_eq!(qty, 0);
    }

    #[test]
    fn discard_sums_same_variant_across_entries() {
        // Same variant selected from two physical locations must collapse into one
        // clamped write-off, not two independent ones.
        let mut conn = test_conn();
        let card = make_card("1", "Bolt", "5");
        sync_inventory_conn(&mut conn, std::slice::from_ref(&card), "2026-01-01").unwrap();

        let stats = discard_cards_conn(&mut conn, &[(card.clone(), 2), (card, 2)]).unwrap();
        assert_eq!(stats.variants_updated, 1);
        assert_eq!(stats.copies_discarded, 4);
        let (qty, _, _) = get_row(&conn, "1").unwrap();
        assert_eq!(qty, 1);
    }

    #[test]
    fn discard_ignores_unknown_variant() {
        let mut conn = test_conn();
        let known = make_card("1", "Bolt", "3");
        sync_inventory_conn(&mut conn, &[known], "2026-01-01").unwrap();

        let ghost = make_card("999", "Nonexistent", "3");
        let stats = discard_cards_conn(&mut conn, &[(ghost, 1)]).unwrap();
        assert_eq!(stats, DiscardStats::default(), "no rows touched");
        let (qty, _, _) = get_row(&conn, "1").unwrap();
        assert_eq!(qty, 3);
    }

    #[test]
    fn discard_matches_new_format_variant_against_legacy_row() {
        // A discard sourced from a new-format inventory-report row (snake_case
        // condition, lowercase language, "false" flags) must still hit the
        // canonical legacy-encoded DB row.
        let mut conn = test_conn();
        let legacy = make_card("42", "Bolt", "4");
        sync_inventory_conn(&mut conn, &[legacy], "2026-01-01").unwrap();

        let mut new_fmt = make_card("42", "Bolt", "0");
        new_fmt.condition = "near_mint".to_string();
        new_fmt.language = "english".to_string();
        new_fmt.is_foil = "false".to_string();
        new_fmt.is_signed = "false".to_string();
        let stats = discard_cards_conn(&mut conn, &[(new_fmt, 1)]).unwrap();

        assert_eq!(stats.variants_updated, 1);
        let (qty, _, _) = get_row(&conn, "42").unwrap();
        assert_eq!(qty, 3);
        assert_eq!(sold_qty(&conn, "42"), 0);
    }

    // ==================== Lot Breakdown Tests ====================

    #[test]
    fn lot_breakdown_groups_by_lot() {
        let mut conn = test_conn();
        let mut c1 = make_card("1", "Bolt", "2");
        c1.price = "1.00".to_string();
        c1.location = Some("A-0-0-1-L1-R".to_string());

        let mut c2 = make_card("2", "Shock", "3");
        c2.price = "0.50".to_string();
        c2.location = Some("A-0-0-2-L1-R".to_string());

        let mut c3 = make_card("3", "Giant Growth", "1");
        c3.price = "5.00".to_string();
        c3.location = Some("B-0-1-1-L2-R".to_string());

        sync_inventory_conn(&mut conn, &[c1, c2, c3], "2026-01-01").unwrap();

        let stats = get_db_stats_conn(&conn, "2026-07-14").unwrap();
        assert_eq!(stats.lot_breakdown.len(), 2);

        let l1 = &stats.lot_breakdown[0];
        assert_eq!(l1.lot, "L1");
        assert_eq!(l1.in_stock_listings, 2);
        assert_eq!(l1.in_stock_copies, 5); // 2 + 3
        assert!((l1.in_stock_value - 3.50).abs() < 0.01); // 1.00*2 + 0.50*3

        let l2 = &stats.lot_breakdown[1];
        assert_eq!(l2.lot, "L2");
        assert_eq!(l2.in_stock_copies, 1);
        assert!((l2.in_stock_value - 5.00).abs() < 0.01);
    }

    #[test]
    fn lot_breakdown_tracks_sold_revenue() {
        let mut conn = test_conn();
        let mut c1 = make_card("1", "Bolt", "4");
        c1.price = "2.00".to_string();
        c1.location = Some("A-0-0-1-L3-R".to_string());

        sync_inventory_conn(&mut conn, &[c1], "2026-01-01").unwrap();

        // All 4 copies sold
        sync_inventory_conn(&mut conn, &[], "2026-01-02").unwrap();

        let stats = get_db_stats_conn(&conn, "2026-07-14").unwrap();
        assert_eq!(stats.lot_breakdown.len(), 1);
        let l3 = &stats.lot_breakdown[0];
        assert_eq!(l3.sold_copies, 4);
        assert!((l3.sold_revenue - 8.00).abs() < 0.01); // 2.00 * 4
        assert_eq!(l3.in_stock_copies, 0);
    }

    #[test]
    fn lot_breakdown_skips_l0_catch_all() {
        let mut conn = test_conn();
        let mut c1 = make_card("1", "Bolt", "2");
        c1.location = Some("A-0-0-1-L0-R".to_string());

        let mut c2 = make_card("2", "Shock", "1");
        c2.location = Some("A-0-0-2-L1-R".to_string());

        sync_inventory_conn(&mut conn, &[c1, c2], "2026-01-01").unwrap();

        let stats = get_db_stats_conn(&conn, "2026-07-14").unwrap();
        assert_eq!(stats.lot_breakdown.len(), 1);
        assert_eq!(stats.lot_breakdown[0].lot, "L1");
    }

    #[test]
    fn lot_breakdown_empty_without_locations() {
        let mut conn = test_conn();
        let card = make_card("1", "Bolt", "2");
        sync_inventory_conn(&mut conn, &[card], "2026-01-01").unwrap();

        let stats = get_db_stats_conn(&conn, "2026-07-14").unwrap();
        assert!(stats.lot_breakdown.is_empty());
    }

    #[test]
    fn lot_breakdown_skips_location_without_lot() {
        let mut conn = test_conn();
        let mut card = make_card("1", "Bolt", "2");
        card.location = Some("A-0-1-4".to_string());
        sync_inventory_conn(&mut conn, &[card], "2026-01-01").unwrap();

        let stats = get_db_stats_conn(&conn, "2026-07-14").unwrap();
        assert!(stats.lot_breakdown.is_empty());
    }

    // ==================== lot acquisition cost & margin ====================

    /// Convenience: build a `LotBreakdown` with only the fields the margin math
    /// depends on; other fields are irrelevant to the assertions.
    fn lot_with(cost: Option<f64>, sold_revenue: f64) -> LotBreakdown {
        LotBreakdown {
            lot: "L1".to_string(),
            in_stock_listings: 0,
            in_stock_copies: 0,
            in_stock_value: 0.0,
            sold_copies: 0,
            sold_revenue,
            cost,
        }
    }

    #[test]
    fn margin_fraction_none_without_cost() {
        assert!(lot_with(None, 50.0).realized_margin_fraction().is_none());
    }

    #[test]
    fn margin_fraction_none_for_zero_cost() {
        // Division by zero is undefined, not "infinite margin".
        assert!(lot_with(Some(0.0), 50.0)
            .realized_margin_fraction()
            .is_none());
    }

    #[test]
    fn margin_fraction_profit_and_loss() {
        // Cost 100, revenue 150 → +50%.
        let profit = lot_with(Some(100.0), 150.0)
            .realized_margin_fraction()
            .unwrap();
        assert!((profit - 0.5).abs() < 1e-9);

        // Cost 100, revenue 40 → -60%.
        let loss = lot_with(Some(100.0), 40.0)
            .realized_margin_fraction()
            .unwrap();
        assert!((loss + 0.6).abs() < 1e-9);
    }

    #[test]
    fn recouped_and_cost_to_recoup() {
        let paid = lot_with(Some(100.0), 100.0);
        assert_eq!(paid.is_recouped(), Some(true));
        assert_eq!(paid.cost_to_recoup(), Some(0.0));

        let over = lot_with(Some(100.0), 130.0);
        assert_eq!(over.is_recouped(), Some(true));
        assert_eq!(over.cost_to_recoup(), Some(0.0)); // never negative

        let under = lot_with(Some(100.0), 60.0);
        assert_eq!(under.is_recouped(), Some(false));
        assert_eq!(under.cost_to_recoup(), Some(40.0));

        let none = lot_with(None, 60.0);
        assert_eq!(none.is_recouped(), None);
        assert_eq!(none.cost_to_recoup(), None);
    }

    #[test]
    fn set_lot_cost_inserts_then_updates() {
        let conn = test_conn();
        set_lot_cost_conn(&conn, "L5", 42.50, "2026-01-01").unwrap();
        let cost = lot_costs_map(&conn).unwrap();
        assert_eq!(cost.get("L5").copied(), Some(42.50));

        // Correcting the buy price overwrites in place (no duplicate row).
        set_lot_cost_conn(&conn, "L5", 30.00, "2026-01-02").unwrap();
        let cost = lot_costs_map(&conn).unwrap();
        assert_eq!(cost.len(), 1);
        assert_eq!(cost.get("L5").copied(), Some(30.00));

        let updated_at: String = conn
            .query_row(
                "SELECT updated_at FROM lot_costs WHERE lot = 'L5'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(updated_at, "2026-01-02");
    }

    #[test]
    fn delete_lot_cost_removes_entry() {
        let conn = test_conn();
        set_lot_cost_conn(&conn, "L5", 42.50, "2026-01-01").unwrap();
        delete_lot_cost_conn(&conn, "L5").unwrap();
        assert!(lot_costs_map(&conn).unwrap().is_empty());
        // Deleting a non-existent lot is a no-op, not an error.
        delete_lot_cost_conn(&conn, "L5").unwrap();
    }

    #[test]
    fn lot_breakdown_populates_recorded_cost() {
        let mut conn = test_conn();
        let mut c1 = make_card("1", "Bolt", "4");
        c1.price = "2.00".to_string();
        c1.location = Some("A-0-0-1-L3-R".to_string());
        sync_inventory_conn(&mut conn, &[c1], "2026-01-01").unwrap();
        set_lot_cost_conn(&conn, "L3", 5.00, "2026-01-01").unwrap();

        let stats = get_db_stats_conn(&conn, "2026-07-14").unwrap();
        let l3 = &stats.lot_breakdown[0];
        assert_eq!(l3.cost, Some(5.00));
        // Revenue is 0 so far → margin -100%, still €5 to recoup.
        assert_eq!(l3.is_recouped(), Some(false));
        assert_eq!(l3.cost_to_recoup(), Some(5.00));
    }

    #[test]
    fn lot_breakdown_cost_is_none_when_unrecorded() {
        let mut conn = test_conn();
        let mut c1 = make_card("1", "Bolt", "2");
        c1.location = Some("A-0-0-1-L1-R".to_string());
        sync_inventory_conn(&mut conn, &[c1], "2026-01-01").unwrap();

        let stats = get_db_stats_conn(&conn, "2026-07-14").unwrap();
        assert_eq!(stats.lot_breakdown[0].cost, None);
    }

    // ==================== snapshots / velocity / in-stock ====================

    #[test]
    fn sync_writes_daily_snapshot() {
        let mut conn = test_conn();
        let mut a = make_card("1", "Bolt", "4");
        a.price = "2.00".to_string();
        sync_inventory_conn(&mut conn, &[a], "2026-01-01").unwrap();

        let snaps = read_snapshots_conn(&conn).unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].date, "2026-01-01");
        assert_eq!(snaps[0].in_stock_copies, 4);
        assert!((snaps[0].in_stock_value - 8.0).abs() < 0.001);
        assert_eq!(snaps[0].sold_copies_cumulative, 0);
    }

    #[test]
    fn same_day_resync_overwrites_snapshot() {
        let mut conn = test_conn();
        sync_inventory_conn(&mut conn, &[make_card("1", "Bolt", "4")], "2026-01-01").unwrap();
        sync_inventory_conn(&mut conn, &[make_card("1", "Bolt", "2")], "2026-01-01").unwrap();

        let snaps = read_snapshots_conn(&conn).unwrap();
        assert_eq!(snaps.len(), 1, "one row per day");
        assert_eq!(snaps[0].in_stock_copies, 2);
    }

    #[test]
    fn snapshot_tracks_cumulative_sales() {
        let mut conn = test_conn();
        let mut c = make_card("1", "Bolt", "10");
        c.price = "1.00".to_string();
        sync_inventory_conn(&mut conn, &[c.clone()], "2026-01-01").unwrap();

        // 4 copies sold by day 2.
        let mut c2 = c.clone();
        c2.quantity = "6".to_string();
        sync_inventory_conn(&mut conn, &[c2], "2026-01-08").unwrap();

        let snaps = read_snapshots_conn(&conn).unwrap();
        assert_eq!(snaps.len(), 2);
        assert_eq!(snaps[1].sold_copies_cumulative, 4);
        assert!((snaps[1].sold_revenue_cumulative - 4.0).abs() < 0.001);
    }

    #[test]
    fn compute_velocity_none_with_single_snapshot() {
        let snaps = vec![InventorySnapshot {
            date: "2026-01-01".to_string(),
            in_stock_copies: 10,
            in_stock_value: 10.0,
            sold_copies_cumulative: 0,
            sold_revenue_cumulative: 0.0,
        }];
        assert!(compute_velocity(&snaps).is_none());
    }

    #[test]
    fn compute_velocity_rates_over_period() {
        let snaps = vec![
            InventorySnapshot {
                date: "2026-01-01".to_string(),
                in_stock_copies: 100,
                in_stock_value: 100.0,
                sold_copies_cumulative: 0,
                sold_revenue_cumulative: 0.0,
            },
            InventorySnapshot {
                date: "2026-01-15".to_string(), // 14 days = 2 weeks
                in_stock_copies: 86,
                in_stock_value: 86.0,
                sold_copies_cumulative: 14,
                sold_revenue_cumulative: 28.0,
            },
        ];
        let v = compute_velocity(&snaps).unwrap();
        assert_eq!(v.period_days, 14);
        assert_eq!(v.sold_copies, 14);
        assert!((v.copies_per_week - 7.0).abs() < 0.001);
        assert!((v.revenue_per_week - 14.0).abs() < 0.001);
    }

    #[test]
    fn compute_velocity_trailing_windows() {
        let snaps = vec![
            InventorySnapshot {
                date: "2026-01-01".to_string(),
                in_stock_copies: 0,
                in_stock_value: 0.0,
                sold_copies_cumulative: 0,
                sold_revenue_cumulative: 0.0,
            },
            InventorySnapshot {
                date: "2026-02-01".to_string(), // ~31 days before latest
                in_stock_copies: 0,
                in_stock_value: 0.0,
                sold_copies_cumulative: 50,
                sold_revenue_cumulative: 50.0,
            },
            InventorySnapshot {
                date: "2026-03-04".to_string(), // latest
                in_stock_copies: 0,
                in_stock_value: 0.0,
                sold_copies_cumulative: 90,
                sold_revenue_cumulative: 90.0,
            },
        ];
        let v = compute_velocity(&snaps).unwrap();
        // last30: nearest snapshot on/before (2026-03-04 - 30d = 2026-02-02) is
        // the 2026-02-01 row (cum 50) → 90 - 50 = 40.
        assert_eq!(v.last30_copies, Some(40));
    }

    #[test]
    fn get_in_stock_cards_excludes_zeroed_and_parses_foil() {
        let mut conn = test_conn();
        let mut foil = make_card("1", "Bolt", "3");
        foil.is_foil = "1".to_string();
        foil.location = Some("A-0-1-1-L2-R".to_string());
        let sold_out = make_card("2", "Shock", "0"); // zero qty, excluded
        sync_inventory_conn(&mut conn, &[foil, sold_out], "2026-01-01").unwrap();

        let cards = get_in_stock_cards_conn(&conn).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].cardmarket_id, "1");
        assert!(cards[0].is_foil);
        assert_eq!(cards[0].quantity, 3);
        assert_eq!(cards[0].location, "A-0-1-1-L2-R");
        // effective_date falls back to listed_at ("2026-01-01" from make_card).
        assert_eq!(cards[0].effective_date, "2026-01-01");
    }

    #[test]
    fn aging_buckets_present_in_stats() {
        let mut conn = test_conn();
        let mut old = make_card("1", "Bolt", "2");
        old.listed_at = "2024-01-01".to_string(); // very old
        sync_inventory_conn(&mut conn, &[old], "2026-07-14").unwrap();

        let stats = get_db_stats_conn(&conn, "2026-07-14").unwrap();
        assert_eq!(stats.aging_buckets.len(), 5);
        assert_eq!(stats.aging_buckets[4].copies, 2, "old card in 365+ bucket");
    }

    // ==================== Sold-Event Recording Tests ====================

    #[test]
    fn sync_records_sold_event_on_partial_sale() {
        let mut conn = test_conn();
        let mut card = make_card("1", "Bolt", "5");
        card.price = "2.50".to_string();
        sync_inventory_conn(&mut conn, &[card], "2026-01-01").unwrap();

        // Qty drops 5 → 2; the new CSV carries a raised price, but the sale must
        // be valued at the price the copies were listed at when they sold.
        let mut card2 = make_card("1", "Bolt", "2");
        card2.price = "4.00".to_string();
        sync_inventory_conn(&mut conn, &[card2], "2026-01-02").unwrap();

        let events = get_sold_events_conn(&conn).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].date, "2026-01-02");
        assert_eq!(events[0].cardmarket_id, "1");
        assert_eq!(events[0].copies, 3);
        assert_eq!(events[0].price, 2.50);
    }

    #[test]
    fn sync_records_sold_event_on_zeroing() {
        let mut conn = test_conn();
        let mut card = make_card("1", "Bolt", "3");
        card.price = "2.00".to_string();
        sync_inventory_conn(&mut conn, &[card], "2026-01-01").unwrap();

        // Card vanished from the CSV → all 3 copies sold.
        sync_inventory_conn(&mut conn, &[], "2026-01-05").unwrap();

        let events = get_sold_events_conn(&conn).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].date, "2026-01-05");
        assert_eq!(events[0].copies, 3);
        assert_eq!(events[0].price, 2.00);
    }

    #[test]
    fn sync_records_no_event_for_new_restocked_or_unchanged() {
        let mut conn = test_conn();
        let card = make_card("1", "Bolt", "2");
        sync_inventory_conn(&mut conn, &[card], "2026-01-01").unwrap();

        // Restock (2 → 5), plus a brand-new variant, plus unchanged next sync.
        let card2 = make_card("1", "Bolt", "5");
        let new_card = make_card("2", "Shock", "4");
        sync_inventory_conn(&mut conn, &[card2.clone(), new_card.clone()], "2026-01-02").unwrap();
        sync_inventory_conn(&mut conn, &[card2, new_card], "2026-01-03").unwrap();

        assert!(get_sold_events_conn(&conn).unwrap().is_empty());
    }

    #[test]
    fn sold_events_accumulate_as_deltas_across_syncs() {
        let mut conn = test_conn();
        sync_inventory_conn(&mut conn, &[make_card("1", "Bolt", "10")], "2026-01-01").unwrap();
        sync_inventory_conn(&mut conn, &[make_card("1", "Bolt", "7")], "2026-01-02").unwrap();
        sync_inventory_conn(&mut conn, &[make_card("1", "Bolt", "5")], "2026-01-03").unwrap();

        let events = get_sold_events_conn(&conn).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].copies, 3);
        assert_eq!(events[1].copies, 2);
        let total: i64 = events.iter().map(|e| e.copies).sum();
        assert_eq!(total, sold_qty(&conn, "1"), "events mirror sold_quantity");
    }

    #[test]
    fn discard_records_no_sold_event() {
        let mut conn = test_conn();
        let card = make_card("1", "Bolt", "10");
        sync_inventory_conn(&mut conn, std::slice::from_ref(&card), "2026-01-01").unwrap();

        discard_cards_conn(&mut conn, &[(card, 4)]).unwrap();

        assert!(
            get_sold_events_conn(&conn).unwrap().is_empty(),
            "write-offs are not sales"
        );
    }

    // ==================== Restock Candidate Tests ====================

    #[test]
    fn restock_candidates_only_sold_out_variants_with_sales() {
        let mut conn = test_conn();
        let sold_out = make_card("1", "Bolt", "3");
        let still_stocked = make_card("2", "Shock", "5");
        let never_sold = make_card("3", "Opt", "0");
        sync_inventory_conn(
            &mut conn,
            &[sold_out, still_stocked.clone(), never_sold.clone()],
            "2026-01-01",
        )
        .unwrap();
        // Bolt sells out; Shock only drops to 4 (still in stock); Opt never sold.
        let partial = make_card("2", "Shock", "4");
        sync_inventory_conn(&mut conn, &[partial, never_sold], "2026-01-10").unwrap();

        let cands = get_restock_candidates_conn(&conn).unwrap();
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].cardmarket_id, "1");
        assert_eq!(cands[0].sold_copies, 3);
        // make_card lists at 2026-01-01; the zeroing sync dates the sale.
        assert_eq!(cands[0].listed_date, "2026-01-01");
        assert_eq!(cands[0].sold_out_date, "2026-01-10");
        assert_eq!(cands[0].realized_revenue, 3.0, "3 copies × €1.00");
    }

    #[test]
    fn restock_revenue_tops_up_copies_sold_before_event_log() {
        let mut conn = test_conn();
        let mut card = make_card("1", "Bolt", "0");
        card.price = "2.00".to_string();
        sync_inventory_conn(&mut conn, &[card], "2026-01-01").unwrap();
        // Simulate pre-event-log history: 5 copies sold with no sold_events rows.
        conn.execute(
            "UPDATE inventory_cards SET sold_quantity = 5 WHERE cardmarket_id = '1'",
            [],
        )
        .unwrap();

        let cands = get_restock_candidates_conn(&conn).unwrap();
        assert_eq!(cands.len(), 1);
        assert_eq!(
            cands[0].realized_revenue, 10.0,
            "untracked copies valued at last listed price"
        );
        assert_eq!(
            cands[0].sold_out_date, "2026-01-01",
            "falls back to last_synced_at without events"
        );
    }
}
