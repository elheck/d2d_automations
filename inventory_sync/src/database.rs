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
    let result: rusqlite::Result<String> =
        conn.query_row("SELECT MAX(price_date) FROM price_history", [], |row| {
            row.get(0)
        });

    match result {
        Ok(date) => Ok(Some(date)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Check if price data exists for today's date
pub fn has_price_data_for_today(conn: &Connection) -> DbResult<bool> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM price_history WHERE price_date = ?1",
        params![&today],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Get today's date as YYYY-MM-DD string
pub fn today_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Get total count of products in database
pub fn get_product_count(conn: &Connection) -> DbResult<i64> {
    conn.query_row("SELECT COUNT(*) FROM products", [], |row| row.get(0))
}

/// Get total count of price history entries
pub fn get_price_history_count(conn: &Connection) -> DbResult<i64> {
    conn.query_row("SELECT COUNT(*) FROM price_history", [], |row| row.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_date_from_timestamp() {
        assert_eq!(extract_date("2026-02-01T02:42:53+0100"), "2026-02-01");
        assert_eq!(extract_date("2025-12-31T23:59:59Z"), "2025-12-31");
    }

    #[test]
    fn extract_date_handles_short_input() {
        assert_eq!(extract_date("2026-02"), "2026-02");
        assert_eq!(extract_date(""), "");
    }
}
