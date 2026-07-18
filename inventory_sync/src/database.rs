//! Database operations for inventory sync
//!
//! Uses parameterized queries exclusively for security (no SQL string concatenation).
//! All writes are transactional for safe shutdown.

use crate::cardmarket::{PriceGuide, ProductCatalog};
use rusqlite::{params, Connection, Transaction};

/// Result type for database operations
pub type DbResult<T> = rusqlite::Result<T>;

/// Initialize the database schema
///
/// Creates tables if they don't exist:
/// - `products`: Product catalog with names and metadata
/// - `price_history`: Daily price snapshots (historical data)
pub fn init_schema(conn: &Connection) -> DbResult<()> {
    conn.execute_batch(
        "
        -- Product catalog table
        CREATE TABLE IF NOT EXISTS products (
            id_product INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            id_category INTEGER NOT NULL,
            category_name TEXT NOT NULL,
            id_expansion INTEGER NOT NULL,
            id_metacard INTEGER NOT NULL,
            date_added TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_products_category ON products(id_category);
        CREATE INDEX IF NOT EXISTS idx_products_expansion ON products(id_expansion);
        CREATE INDEX IF NOT EXISTS idx_products_metacard ON products(id_metacard);

        -- Historical price data table
        -- Composite primary key: (id_product, price_date) ensures one entry per product per day
        CREATE TABLE IF NOT EXISTS price_history (
            id_product INTEGER NOT NULL,
            price_date TEXT NOT NULL,
            id_category INTEGER NOT NULL,
            avg REAL,
            low REAL,
            trend REAL,
            avg1 REAL,
            avg7 REAL,
            avg30 REAL,
            avg_foil REAL,
            low_foil REAL,
            trend_foil REAL,
            avg1_foil REAL,
            avg7_foil REAL,
            avg30_foil REAL,
            created_at TEXT NOT NULL,
            inserted_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (id_product, price_date),
            FOREIGN KEY (id_product) REFERENCES products(id_product)
        );

        CREATE INDEX IF NOT EXISTS idx_price_history_date ON price_history(price_date);
        CREATE INDEX IF NOT EXISTS idx_price_history_product ON price_history(id_product);

        -- Expansion name cache populated from Scryfall lookups
        CREATE TABLE IF NOT EXISTS expansion_names (
            id_expansion INTEGER PRIMARY KEY,
            name TEXT NOT NULL
        );

        -- The buy-signal scanner was removed (its daily scan cost too much CPU
        -- on the server); drop its leftover tables from older deployments.
        DROP TABLE IF EXISTS buy_signals;
        DROP TABLE IF EXISTS buy_signals_meta;
        ",
    )?;

    log::info!("Database schema initialized");
    Ok(())
}

/// Upsert products from the catalog into the database
///
/// Uses INSERT OR REPLACE to update existing products with new data.
/// All operations are wrapped in a transaction for atomicity.
pub fn upsert_products(conn: &mut Connection, catalog: &ProductCatalog) -> DbResult<usize> {
    let tx = conn.transaction()?;
    let count = upsert_products_tx(&tx, catalog)?;
    tx.commit()?;
    Ok(count)
}

fn upsert_products_tx(tx: &Transaction<'_>, catalog: &ProductCatalog) -> DbResult<usize> {
    let mut stmt = tx.prepare_cached(
        "INSERT OR REPLACE INTO products 
         (id_product, name, id_category, category_name, id_expansion, id_metacard, date_added, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))",
    )?;

    let mut count = 0;
    for product in catalog.iter() {
        stmt.execute(params![
            product.id_product,
            &product.name,
            product.id_category,
            &product.category_name,
            product.id_expansion,
            product.id_metacard,
            &product.date_added,
        ])?;
        count += 1;
    }

    log::info!("Upserted {} products into database", count);
    Ok(count)
}

/// Store an expansion name learned from a Scryfall lookup.
///
/// Uses INSERT OR IGNORE — first name wins, existing entries are not overwritten.
pub fn upsert_expansion_name(conn: &Connection, id_expansion: u64, name: &str) -> DbResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO expansion_names (id_expansion, name) VALUES (?1, ?2)",
        params![id_expansion, name],
    )?;
    Ok(())
}

/// Look up the expansion ID for a given product.
pub fn get_id_expansion_for_product(conn: &Connection, id_product: u64) -> DbResult<Option<u64>> {
    let mut stmt = conn.prepare("SELECT id_expansion FROM products WHERE id_product = ?1")?;
    let mut rows = stmt.query(params![id_product])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

/// Insert price history for a specific date
///
/// Only inserts prices for products that don't already have data for this date.
/// This preserves historical data and avoids duplicate entries.
///
/// Returns the number of new price entries inserted.
pub fn insert_price_history(
    conn: &mut Connection,
    guide: &PriceGuide,
    catalog: &ProductCatalog,
) -> DbResult<InsertResult> {
    let tx = conn.transaction()?;
    let result = insert_price_history_tx(&tx, guide, catalog)?;
    tx.commit()?;
    Ok(result)
}

/// Result of a price history insert operation
#[derive(Debug)]
pub struct InsertResult {
    /// Number of new entries inserted
    pub inserted: usize,
    /// Number of entries skipped (already existed for this date)
    pub skipped: usize,
    /// Number of entries with no matching product in catalog
    pub no_product: usize,
    /// The price date used
    pub price_date: String,
}

fn insert_price_history_tx(
    tx: &Transaction<'_>,
    guide: &PriceGuide,
    catalog: &ProductCatalog,
) -> DbResult<InsertResult> {
    // Extract date from created_at (format: "2026-02-01T02:42:53+0100")
    let price_date = extract_date(guide.created_at());

    // Check if we already have data for this date
    let existing_count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM price_history WHERE price_date = ?1",
        params![&price_date],
        |row| row.get(0),
    )?;

    if existing_count > 0 {
        log::info!(
            "Price data for {} already exists ({} entries), skipping insert",
            price_date,
            existing_count
        );
        return Ok(InsertResult {
            inserted: 0,
            skipped: guide.len(),
            no_product: 0,
            price_date,
        });
    }

    // Prepare the insert statement (parameterized for security)
    let mut stmt = tx.prepare_cached(
        "INSERT INTO price_history 
         (id_product, price_date, id_category, avg, low, trend, avg1, avg7, avg30,
          avg_foil, low_foil, trend_foil, avg1_foil, avg7_foil, avg30_foil, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
    )?;

    let mut inserted = 0;
    let mut no_product = 0;

    for entry in guide.iter() {
        // Only insert if product exists in catalog (ensures data integrity)
        if catalog.get(entry.id_product).is_some() {
            stmt.execute(params![
                entry.id_product,
                &price_date,
                entry.id_category,
                entry.avg,
                entry.low,
                entry.trend,
                entry.avg1,
                entry.avg7,
                entry.avg30,
                entry.avg_foil,
                entry.low_foil,
                entry.trend_foil,
                entry.avg1_foil,
                entry.avg7_foil,
                entry.avg30_foil,
                guide.created_at(),
            ])?;
            inserted += 1;
        } else {
            no_product += 1;
        }
    }

    log::info!(
        "Inserted {} price entries for {} ({} products not in catalog)",
        inserted,
        price_date,
        no_product
    );

    Ok(InsertResult {
        inserted,
        skipped: 0,
        no_product,
        price_date,
    })
}

/// Extract date (YYYY-MM-DD) from a timestamp string
///
/// Expected format: "2026-02-01T02:42:53+0100"
fn extract_date(timestamp: &str) -> String {
    // Take first 10 characters (YYYY-MM-DD)
    timestamp.chars().take(10).collect()
}

/// Get the latest price date in the database
pub fn get_latest_price_date(conn: &Connection) -> DbResult<Option<String>> {
    let mut stmt = conn.prepare("SELECT MAX(price_date) FROM price_history")?;
    let mut rows = stmt.query([])?;

    match rows.next()? {
        Some(row) => {
            let date: Option<String> = row.get(0)?;
            Ok(date)
        }
        None => Ok(None),
    }
}

/// Check if price data exists for today's date (Berlin timezone)
///
/// Uses Europe/Berlin timezone because Cardmarket timestamps are in Berlin time.
/// The server may run in a different timezone, so we must be explicit.
pub fn has_price_data_for_today(conn: &Connection) -> DbResult<bool> {
    let today = today_date();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM price_history WHERE price_date = ?1",
        params![&today],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Get today's date as YYYY-MM-DD string in Berlin timezone
///
/// Uses Europe/Berlin because Cardmarket data is timestamped in Berlin time.
pub fn today_date() -> String {
    use chrono::Utc;
    use chrono_tz::Europe::Berlin;
    Utc::now()
        .with_timezone(&Berlin)
        .format("%Y-%m-%d")
        .to_string()
}

/// Get total count of products in database
pub fn get_product_count(conn: &Connection) -> DbResult<i64> {
    conn.query_row("SELECT COUNT(*) FROM products", [], |row| row.get(0))
}

/// Get total count of price history entries
pub fn get_price_history_count(conn: &Connection) -> DbResult<i64> {
    conn.query_row("SELECT COUNT(*) FROM price_history", [], |row| row.get(0))
}

// ── Web API Query Functions ────────────────────────────────────────────────

// Wire types shared with client apps live in mtg_common; re-exported here so
// the rest of the crate keeps using `crate::database::…` paths.
pub use mtg_common::inventory_sync::{
    LatestPrice, PriceHistoryPoint, PriceSnapshot, ProductSearchResult,
};

/// Search products by name (case-insensitive substring match)
///
/// Returns up to `limit` results, prioritizing exact name matches first,
/// then partial matches, all ordered alphabetically.
pub fn search_products_by_name(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> DbResult<Vec<ProductSearchResult>> {
    let pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT p.id_product, p.name, p.category_name, p.id_expansion, e.name
         FROM products p
         LEFT JOIN expansion_names e ON p.id_expansion = e.id_expansion
         WHERE p.name LIKE ?1 COLLATE NOCASE
         ORDER BY
             CASE WHEN p.name = ?2 COLLATE NOCASE THEN 0
                  WHEN p.name LIKE ?2 COLLATE NOCASE THEN 1
                  ELSE 2
             END,
             p.name
         LIMIT ?3",
    )?;

    let results: DbResult<Vec<ProductSearchResult>> = stmt
        .query_map(params![pattern, query, limit], |row| {
            Ok(ProductSearchResult {
                id_product: row.get(0)?,
                name: row.get(1)?,
                category_name: row.get(2)?,
                id_expansion: row.get(3)?,
                expansion_name: row.get(4)?,
            })
        })?
        .collect();
    results
}

/// Get price history for a product, optionally filtered to dates on or after `since_date`.
///
/// `since_date` must be an ISO date string (`YYYY-MM-DD`). Pass `None` to return all history.
pub fn get_price_history(
    conn: &Connection,
    id_product: u64,
    since_date: Option<&str>,
) -> DbResult<Vec<PriceHistoryPoint>> {
    let map_row = |row: &rusqlite::Row| {
        Ok(PriceHistoryPoint {
            price_date: row.get(0)?,
            avg: row.get(1)?,
            low: row.get(2)?,
            trend: row.get(3)?,
            avg1: row.get(4)?,
            avg7: row.get(5)?,
            avg30: row.get(6)?,
            avg_foil: row.get(7)?,
            low_foil: row.get(8)?,
            trend_foil: row.get(9)?,
            avg1_foil: row.get(10)?,
            avg7_foil: row.get(11)?,
            avg30_foil: row.get(12)?,
        })
    };

    match since_date {
        Some(date) => {
            let mut stmt = conn.prepare(
                "SELECT price_date, avg, low, trend, avg1, avg7, avg30,
                        avg_foil, low_foil, trend_foil, avg1_foil, avg7_foil, avg30_foil
                 FROM price_history
                 WHERE id_product = ?1 AND price_date >= ?2
                 ORDER BY price_date ASC",
            )?;
            let rows = stmt.query_map(params![id_product, date], map_row)?;
            rows.collect()
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT price_date, avg, low, trend, avg1, avg7, avg30,
                        avg_foil, low_foil, trend_foil, avg1_foil, avg7_foil, avg30_foil
                 FROM price_history
                 WHERE id_product = ?1
                 ORDER BY price_date ASC",
            )?;
            let rows = stmt.query_map(params![id_product], map_row)?;
            rows.collect()
        }
    }
}

/// Get the latest price row for each of the given product IDs.
///
/// Uses a parameterized query per product (SQLite has no native array binding).
/// Returns only products that have at least one price_history row.
pub fn get_latest_prices_bulk(conn: &Connection, ids: &[u64]) -> DbResult<Vec<LatestPrice>> {
    let mut results = Vec::with_capacity(ids.len());
    let mut stmt = conn.prepare(
        "SELECT id_product, price_date, avg, low, trend, avg1, avg7, avg30,
                avg_foil, low_foil, trend_foil, avg1_foil, avg7_foil, avg30_foil
         FROM price_history
         WHERE id_product = ?1
         ORDER BY price_date DESC
         LIMIT 1",
    )?;
    for &id in ids {
        if let Some(row) = stmt
            .query_map(params![id], |row| {
                Ok(LatestPrice {
                    id_product: row.get(0)?,
                    price_date: row.get(1)?,
                    avg: row.get(2)?,
                    low: row.get(3)?,
                    trend: row.get(4)?,
                    avg1: row.get(5)?,
                    avg7: row.get(6)?,
                    avg30: row.get(7)?,
                    avg_foil: row.get(8)?,
                    low_foil: row.get(9)?,
                    trend_foil: row.get(10)?,
                    avg1_foil: row.get(11)?,
                    avg7_foil: row.get(12)?,
                    avg30_foil: row.get(13)?,
                })
            })?
            .next()
        {
            results.push(row?);
        }
    }
    Ok(results)
}

/// Get the price row in effect on each requested date for each product.
///
/// For every (id, date) pair this runs one indexed lookup: the most recent
/// `price_history` row with `price_date <= date`. Deliberately no aggregation
/// here — clients compute deltas themselves to keep server load minimal.
/// Pairs with no data on or before the date are omitted from the result.
pub fn get_price_snapshots_bulk(
    conn: &Connection,
    ids: &[u64],
    dates: &[String],
) -> DbResult<Vec<PriceSnapshot>> {
    let mut results = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT price_date, avg, low, trend, avg1, avg7, avg30,
                avg_foil, low_foil, trend_foil, avg1_foil, avg7_foil, avg30_foil
         FROM price_history
         WHERE id_product = ?1 AND price_date <= ?2
         ORDER BY price_date DESC
         LIMIT 1",
    )?;
    for &id in ids {
        for date in dates {
            if let Some(row) = stmt
                .query_map(params![id, date], |row| {
                    Ok(PriceSnapshot {
                        id_product: id,
                        requested_date: date.clone(),
                        price_date: row.get(0)?,
                        avg: row.get(1)?,
                        low: row.get(2)?,
                        trend: row.get(3)?,
                        avg1: row.get(4)?,
                        avg7: row.get(5)?,
                        avg30: row.get(6)?,
                        avg_foil: row.get(7)?,
                        low_foil: row.get(8)?,
                        trend_foil: row.get(9)?,
                        avg1_foil: row.get(10)?,
                        avg7_foil: row.get(11)?,
                        avg30_foil: row.get(12)?,
                    })
                })?
                .next()
            {
                results.push(row?);
            }
        }
    }
    Ok(results)
}

/// Get product details by ID
pub fn get_product_by_id(
    conn: &Connection,
    id_product: u64,
) -> DbResult<Option<ProductSearchResult>> {
    let mut stmt = conn.prepare(
        "SELECT p.id_product, p.name, p.category_name, p.id_expansion, e.name
         FROM products p
         LEFT JOIN expansion_names e ON p.id_expansion = e.id_expansion
         WHERE p.id_product = ?1",
    )?;

    let mut rows = stmt.query(params![id_product])?;
    match rows.next()? {
        Some(row) => Ok(Some(ProductSearchResult {
            id_product: row.get(0)?,
            name: row.get(1)?,
            category_name: row.get(2)?,
            id_expansion: row.get(3)?,
            expansion_name: row.get(4)?,
        })),
        None => Ok(None),
    }
}

#[cfg(test)]
#[path = "database_tests.rs"]
mod tests;
