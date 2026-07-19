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

/// Read-only preview of what a sync would change — computed before any write
/// so a bad CSV can be caught first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncPreview {
    /// Variants in the CSV not yet in the DB.
    pub new_variants: usize,
    /// Variants present in both CSV and DB.
    pub updated_variants: usize,
    /// In-stock variants that vanished from the CSV (would be zeroed).
    pub zeroed_variants: usize,
    /// Copies that would be recorded as sold (quantity drops + vanished rows).
    pub copies_sold: i64,
    /// Variants whose listed price would change.
    pub price_changes: usize,
    /// Total in-stock copies before / after the sync.
    pub copies_before: i64,
    pub copies_after: i64,
}

/// Guard threshold: the safety check only engages on inventories of at least
/// this many copies (tiny/fresh DBs churn legitimately).
pub const MIN_COPIES_FOR_GUARD: i64 = 100;

/// A sync recording more than this share of the inventory as sold in one go is
/// treated as a suspect CSV rather than real sales.
pub const SUSPICIOUS_DROP_FRACTION: f64 = 0.5;

impl SyncPreview {
    /// True when the sync would wipe out a large share of a non-trivial
    /// inventory at once — more likely a truncated or wrong CSV than sales.
    pub fn is_suspicious(&self) -> bool {
        self.copies_before >= MIN_COPIES_FOR_GUARD
            && self.copies_sold as f64 > self.copies_before as f64 * SUSPICIOUS_DROP_FRACTION
    }
}

/// Result of a guarded sync: either it ran, or the safety check blocked it and
/// nothing was written (rerun via [`sync_inventory_forced`] to override).
#[derive(Debug)]
pub enum SyncOutcome {
    Synced(SyncStats),
    Blocked(SyncPreview),
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
// Small key-value store for app bookkeeping (visit dates for the welcome
// digest). Not card data — never touched by syncs.
const APP_META_DDL: &str = "
    CREATE TABLE IF NOT EXISTS app_meta (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
";

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
        conn.execute_batch(LOT_COSTS_DDL)?;
        return conn.execute_batch(APP_META_DDL);
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
    conn.execute_batch(APP_META_DDL)?;

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

/// Syncs a slice of cards (from a freshly loaded inventory CSV) to the local DB,
/// with two safety layers in front of the write:
///
/// 1. A dated backup of the DB file is taken (first write of the day) so a bad
///    import can always be rolled back.
/// 2. The change is previewed first; a sync that would record most of the
///    inventory as sold is blocked and returned as [`SyncOutcome::Blocked`]
///    with nothing written — the caller decides whether to
///    [`sync_inventory_forced`].
///
/// Sync semantics:
/// - Cards are pre-aggregated by variant (cardmarket_id + condition + language + foil +
///   signed): quantities across multiple locations are summed into a single DB row.
/// - Existing variants are updated; new variants are inserted.
/// - Variants not in `cards` that have `quantity > 0` are zeroed out.
/// - Timestamps are only advanced once per day.
///
/// Errors are returned to the caller; call sites treat them as non-fatal.
pub fn sync_inventory(cards: &[Card]) -> DbResult<SyncOutcome> {
    let mut conn = open_db()?;
    backup_db_file(&conn, &today_date());
    let preview = preview_sync_conn(&conn, cards)?;
    if preview.is_suspicious() {
        log::warn!(
            "Inventory sync blocked: would record {} of {} copies as sold ({} variants zeroed)",
            preview.copies_sold,
            preview.copies_before,
            preview.zeroed_variants
        );
        return Ok(SyncOutcome::Blocked(preview));
    }
    Ok(SyncOutcome::Synced(sync_inventory_conn(
        &mut conn,
        cards,
        &today_date(),
    )?))
}

/// Runs the sync without the suspicious-change guard (still takes the daily
/// backup first). Use only after the user has confirmed a blocked sync.
pub fn sync_inventory_forced(cards: &[Card]) -> DbResult<SyncStats> {
    let mut conn = open_db()?;
    backup_db_file(&conn, &today_date());
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

/// Reads one value from the app bookkeeping store.
fn get_meta(conn: &Connection, key: &str) -> DbResult<Option<String>> {
    conn.query_row("SELECT value FROM app_meta WHERE key = ?1", [key], |r| {
        r.get(0)
    })
    .optional()
}

/// Writes one value to the app bookkeeping store.
fn set_meta(conn: &Connection, key: &str, value: &str) -> DbResult<()> {
    conn.execute(
        "INSERT INTO app_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

/// What changed since the previous visit — shown on the welcome screen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VisitDigest {
    /// Baseline date the digest is measured against (the previous day the app
    /// was started); `None` on the very first visit.
    pub since: Option<String>,
    /// Copies recorded as sold since the baseline, and their revenue.
    pub sold_copies: i64,
    pub sold_revenue: f64,
    /// Variants first seen by a sync after the baseline.
    pub new_listings: i64,
    /// Most recent day any variant was synced (proxy for "last CSV import").
    pub last_sync: Option<String>,
    /// Current number of restock candidates (sold out, worth re-buying).
    pub restock_candidates: i64,
}

/// Builds the since-last-visit digest and records today as the latest visit.
///
/// Visits are tracked per day: starting the app again on the same day keeps
/// the same baseline instead of resetting the digest to empty.
pub fn visit_digest() -> DbResult<VisitDigest> {
    let conn = open_db()?;
    visit_digest_conn(&conn, &today_date())
}

/// Inner digest that accepts an explicit connection and date — used in tests.
fn visit_digest_conn(conn: &Connection, today: &str) -> DbResult<VisitDigest> {
    let last = get_meta(conn, "last_visit")?;
    let baseline = match last {
        Some(d) if d != today => {
            set_meta(conn, "prev_visit", &d)?;
            set_meta(conn, "last_visit", today)?;
            Some(d)
        }
        Some(_) => get_meta(conn, "prev_visit")?,
        None => {
            set_meta(conn, "last_visit", today)?;
            None
        }
    };

    let mut digest = VisitDigest {
        since: baseline.clone(),
        ..VisitDigest::default()
    };
    if let Some(since) = &baseline {
        let (copies, revenue): (Option<i64>, Option<f64>) = conn.query_row(
            "SELECT SUM(copies), SUM(copies * price) FROM sold_events WHERE date > ?1",
            [since.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        digest.sold_copies = copies.unwrap_or(0);
        digest.sold_revenue = revenue.unwrap_or(0.0);
        digest.new_listings = conn.query_row(
            "SELECT COUNT(*) FROM inventory_cards WHERE first_synced_at > ?1",
            [since.as_str()],
            |r| r.get(0),
        )?;
    }
    digest.last_sync =
        conn.query_row("SELECT MAX(last_synced_at) FROM inventory_cards", [], |r| {
            r.get(0)
        })?;
    digest.restock_candidates = conn.query_row(
        "SELECT COUNT(*) FROM inventory_cards WHERE quantity = 0 AND sold_quantity > 0",
        [],
        |r| r.get(0),
    )?;
    Ok(digest)
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

/// Pre-aggregates CSV rows by (cardmarket_id, condition, language, is_foil,
/// is_signed), summing quantities. The key components are normalised to the
/// canonical form stored in the DB so the legacy Cardmarket export and the new
/// inventory-report CSV produce identical variant keys (see `normalize_flag` /
/// `normalize_language` / `canonical_condition` for the encodings used).
///
/// The inventory-report CSV emits a placeholder "summary" row (quantity 0,
/// empty location) *before* the real per-location row for a variant; the
/// representative card therefore prefers any row with a non-empty location.
#[allow(clippy::type_complexity)]
fn aggregate_by_variant(
    cards: &[Card],
) -> std::collections::HashMap<(String, String, String, String, String), (&Card, i64)> {
    let mut agg: std::collections::HashMap<(String, String, String, String, String), (&Card, i64)> =
        std::collections::HashMap::new();
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

        let rep_has_loc = card_has_location(entry.0);
        let new_has_loc = card_has_location(card);
        if new_has_loc && !rep_has_loc {
            entry.0 = card;
        }
    }
    agg
}

/// Parses a CSV price string (both `.` and `,` decimal separators).
fn parse_csv_price(price: &str) -> Option<f64> {
    price.trim().replace(',', ".").parse::<f64>().ok()
}

/// Computes what a sync of `cards` would change, without writing anything.
fn preview_sync_conn(conn: &Connection, cards: &[Card]) -> DbResult<SyncPreview> {
    let agg = aggregate_by_variant(cards);
    let mut preview = SyncPreview::default();

    // Current DB state keyed like the sync keys it.
    let mut db: std::collections::HashMap<String, (i64, f64)> = std::collections::HashMap::new();
    let rows = conn
        .prepare(
            "SELECT cardmarket_id, condition, language, is_foil, is_signed,
                    quantity, CAST(price AS REAL) FROM inventory_cards",
        )?
        .query_map([], |row| {
            Ok((
                article_key(
                    &row.get::<_, String>(0)?,
                    &row.get::<_, String>(1)?,
                    &row.get::<_, String>(2)?,
                    &row.get::<_, String>(3)?,
                    &row.get::<_, String>(4)?,
                ),
                row.get::<_, i64>(5)?,
                row.get::<_, f64>(6)?,
            ))
        })?
        .collect::<DbResult<Vec<_>>>()?;
    for (key, qty, price) in rows {
        if qty > 0 {
            preview.copies_before += qty;
        }
        db.insert(key, (qty, price));
    }

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for ((id, cond, lang, foil, signed), (card, qty)) in &agg {
        let key = article_key(id, cond, lang, foil, signed);
        preview.copies_after += *qty;
        match db.get(&key) {
            None => preview.new_variants += 1,
            Some((db_qty, db_price)) => {
                preview.updated_variants += 1;
                if qty < db_qty {
                    preview.copies_sold += db_qty - qty;
                }
                if let Some(p) = parse_csv_price(&card.price) {
                    if (p - db_price).abs() > 0.005 {
                        preview.price_changes += 1;
                    }
                }
            }
        }
        seen.insert(key);
    }

    // In-stock DB variants that vanished from the CSV would be zeroed (and
    // their remaining copies recorded as sold).
    for (key, (qty, _)) in &db {
        if *qty > 0 && !seen.contains(key) {
            preview.zeroed_variants += 1;
            preview.copies_sold += qty;
        }
    }

    Ok(preview)
}

/// How many daily backups of the inventory DB file to keep.
const BACKUP_KEEP: usize = 3;

/// Takes a dated snapshot of the inventory DB next to it
/// (`inventory-YYYY-MM-DD.db.bak`) before the first write of the day, and
/// prunes old backups. Uses SQLite's `VACUUM INTO` so the snapshot is
/// consistent regardless of journal mode. Failures are logged, never fatal —
/// a backup must not block a sync.
fn backup_db_file(conn: &Connection, today: &str) {
    let Some(dir) = db_path().parent().map(std::path::Path::to_path_buf) else {
        return;
    };
    if let Err(e) = backup_db_at(conn, &dir, today, BACKUP_KEEP) {
        log::warn!("Inventory DB backup failed: {e}");
    }
}

/// Inner backup that accepts explicit connection and directory — used in tests.
/// Dated file names sort lexicographically = chronologically, so pruning keeps
/// the `keep` newest.
fn backup_db_at(
    conn: &Connection,
    dir: &std::path::Path,
    today: &str,
    keep: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let dst = dir.join(format!("inventory-{today}.db.bak"));
    if !dst.exists() {
        conn.execute("VACUUM INTO ?1", [dst.to_string_lossy().as_ref()])?;
        log::info!("Inventory DB backed up to {}", dst.display());
    }
    let mut backups: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("inventory-") && n.ends_with(".db.bak"))
        })
        .collect();
    backups.sort();
    while backups.len() > keep {
        let old = backups.remove(0);
        std::fs::remove_file(&old)?;
    }
    Ok(())
}

/// Inner sync that accepts an explicit connection and date — used in tests.
fn sync_inventory_conn(conn: &mut Connection, cards: &[Card], today: &str) -> DbResult<SyncStats> {
    log::debug!("Syncing {} cards to inventory DB ({})", cards.len(), today);
    let tx = conn.transaction()?;
    let mut stats = SyncStats::default();
    let agg = aggregate_by_variant(cards);

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
#[path = "inventory_db_tests.rs"]
mod tests;
