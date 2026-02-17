//! Database operations for inventory sync
//!
//! Uses parameterized queries exclusively for security (no SQL string concatenation).
//! All writes are transactional for safe shutdown.

use crate::cardmarket::{PriceGuide, ProductCatalog};
use rusqlite::{params, Connection, Transaction};
use serde::Serialize;

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

/// Product search result (for API responses)
#[derive(Debug, Clone, Serialize)]
pub struct ProductSearchResult {
    pub id_product: u64,
    pub name: String,
    pub category_name: String,
    pub id_expansion: u64,
}

/// Price history data point (for charting)
#[derive(Debug, Clone, Serialize)]
pub struct PriceHistoryPoint {
    pub price_date: String,
    pub avg: Option<f64>,
    pub low: Option<f64>,
    pub trend: Option<f64>,
    pub avg1: Option<f64>,
    pub avg7: Option<f64>,
    pub avg30: Option<f64>,
    pub avg_foil: Option<f64>,
    pub low_foil: Option<f64>,
    pub trend_foil: Option<f64>,
    pub avg1_foil: Option<f64>,
    pub avg7_foil: Option<f64>,
    pub avg30_foil: Option<f64>,
}

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
        "SELECT id_product, name, category_name, id_expansion
         FROM products
         WHERE name LIKE ?1 COLLATE NOCASE
         ORDER BY
             CASE WHEN name = ?2 COLLATE NOCASE THEN 0
                  WHEN name LIKE ?2 COLLATE NOCASE THEN 1
                  ELSE 2
             END,
             name
         LIMIT ?3",
    )?;

    let results: DbResult<Vec<ProductSearchResult>> = stmt
        .query_map(params![pattern, query, limit], |row| {
            Ok(ProductSearchResult {
                id_product: row.get(0)?,
                name: row.get(1)?,
                category_name: row.get(2)?,
                id_expansion: row.get(3)?,
            })
        })?
        .collect();
    results
}

/// Get price history for a product (all dates, ordered chronologically)
pub fn get_price_history(conn: &Connection, id_product: u64) -> DbResult<Vec<PriceHistoryPoint>> {
    let mut stmt = conn.prepare(
        "SELECT price_date, avg, low, trend, avg1, avg7, avg30,
                avg_foil, low_foil, trend_foil, avg1_foil, avg7_foil, avg30_foil
         FROM price_history
         WHERE id_product = ?1
         ORDER BY price_date ASC",
    )?;

    let results: DbResult<Vec<PriceHistoryPoint>> = stmt
        .query_map(params![id_product], |row| {
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
        })?
        .collect();
    results
}

/// Get product details by ID
pub fn get_product_by_id(
    conn: &Connection,
    id_product: u64,
) -> DbResult<Option<ProductSearchResult>> {
    let mut stmt = conn.prepare(
        "SELECT id_product, name, category_name, id_expansion
         FROM products
         WHERE id_product = ?1",
    )?;

    let mut rows = stmt.query(params![id_product])?;
    match rows.next()? {
        Some(row) => Ok(Some(ProductSearchResult {
            id_product: row.get(0)?,
            name: row.get(1)?,
            category_name: row.get(2)?,
            id_expansion: row.get(3)?,
        })),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cardmarket::{make_test_price_entry, make_test_product, PriceGuide, ProductCatalog};

    /// Create an in-memory database for testing
    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

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

    #[test]
    fn init_schema_creates_tables() {
        let conn = test_db();

        // Verify products table exists
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='products'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Verify price_history table exists
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='price_history'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn upsert_products_inserts_new_products() {
        let mut conn = test_db();
        let catalog = ProductCatalog::from_entries(vec![
            make_test_product(1, "Black Lotus"),
            make_test_product(2, "Mox Pearl"),
        ]);

        let count = upsert_products(&mut conn, &catalog).unwrap();
        assert_eq!(count, 2);

        // Verify products are in database
        let db_count = get_product_count(&conn).unwrap();
        assert_eq!(db_count, 2);

        // Verify product data
        let name: String = conn
            .query_row(
                "SELECT name FROM products WHERE id_product = ?1",
                params![1],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "Black Lotus");
    }

    #[test]
    fn upsert_products_updates_existing() {
        let mut conn = test_db();

        // Insert initial product
        let catalog1 = ProductCatalog::from_entries(vec![make_test_product(1, "Black Lotus")]);
        upsert_products(&mut conn, &catalog1).unwrap();

        // Update with new name
        let catalog2 =
            ProductCatalog::from_entries(vec![make_test_product(1, "Black Lotus (Updated)")]);
        upsert_products(&mut conn, &catalog2).unwrap();

        // Should still be 1 product
        let db_count = get_product_count(&conn).unwrap();
        assert_eq!(db_count, 1);

        // Name should be updated
        let name: String = conn
            .query_row(
                "SELECT name FROM products WHERE id_product = ?1",
                params![1],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "Black Lotus (Updated)");
    }

    #[test]
    fn insert_price_history_inserts_prices() {
        let mut conn = test_db();

        // First insert products
        let catalog = ProductCatalog::from_entries(vec![
            make_test_product(1, "Black Lotus"),
            make_test_product(2, "Mox Pearl"),
        ]);
        upsert_products(&mut conn, &catalog).unwrap();

        // Insert price guide
        let guide = PriceGuide::from_entries(
            vec![
                make_test_price_entry(1, Some(2000.0)),
                make_test_price_entry(2, Some(500.0)),
            ],
            "2026-02-01T10:00:00+0100",
        );

        let result = insert_price_history(&mut conn, &guide, &catalog).unwrap();
        assert_eq!(result.inserted, 2);
        assert_eq!(result.skipped, 0);
        assert_eq!(result.no_product, 0);
        assert_eq!(result.price_date, "2026-02-01");

        // Verify prices in database
        let count = get_price_history_count(&conn).unwrap();
        assert_eq!(count, 2);

        // Verify price data
        let trend: f64 = conn
            .query_row(
                "SELECT trend FROM price_history WHERE id_product = ?1 AND price_date = ?2",
                params![1, "2026-02-01"],
                |row| row.get(0),
            )
            .unwrap();
        assert!((trend - 2000.0).abs() < 0.01);
    }

    #[test]
    fn insert_price_history_skips_duplicate_date() {
        let mut conn = test_db();

        // Insert products
        let catalog = ProductCatalog::from_entries(vec![make_test_product(1, "Black Lotus")]);
        upsert_products(&mut conn, &catalog).unwrap();

        // Insert price guide first time
        let guide = PriceGuide::from_entries(
            vec![make_test_price_entry(1, Some(2000.0))],
            "2026-02-01T10:00:00+0100",
        );
        let result1 = insert_price_history(&mut conn, &guide, &catalog).unwrap();
        assert_eq!(result1.inserted, 1);

        // Try to insert same date again
        let guide2 = PriceGuide::from_entries(
            vec![make_test_price_entry(1, Some(2500.0))], // Different price
            "2026-02-01T15:00:00+0100",                   // Same date, different time
        );
        let result2 = insert_price_history(&mut conn, &guide2, &catalog).unwrap();
        assert_eq!(result2.inserted, 0);
        assert_eq!(result2.skipped, 1);

        // Price should be unchanged (first insert preserved)
        let trend: f64 = conn
            .query_row(
                "SELECT trend FROM price_history WHERE id_product = ?1",
                params![1],
                |row| row.get(0),
            )
            .unwrap();
        assert!((trend - 2000.0).abs() < 0.01);
    }

    #[test]
    fn insert_price_history_allows_different_dates() {
        let mut conn = test_db();

        // Insert products
        let catalog = ProductCatalog::from_entries(vec![make_test_product(1, "Black Lotus")]);
        upsert_products(&mut conn, &catalog).unwrap();

        // Insert for day 1
        let guide1 = PriceGuide::from_entries(
            vec![make_test_price_entry(1, Some(2000.0))],
            "2026-02-01T10:00:00+0100",
        );
        insert_price_history(&mut conn, &guide1, &catalog).unwrap();

        // Insert for day 2
        let guide2 = PriceGuide::from_entries(
            vec![make_test_price_entry(1, Some(2100.0))],
            "2026-02-02T10:00:00+0100",
        );
        let result = insert_price_history(&mut conn, &guide2, &catalog).unwrap();
        assert_eq!(result.inserted, 1);

        // Should have 2 entries (historical data preserved)
        let count = get_price_history_count(&conn).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn insert_price_history_skips_products_not_in_catalog() {
        let mut conn = test_db();

        // Insert only one product
        let catalog = ProductCatalog::from_entries(vec![make_test_product(1, "Black Lotus")]);
        upsert_products(&mut conn, &catalog).unwrap();

        // Price guide has entry for product not in catalog
        let guide = PriceGuide::from_entries(
            vec![
                make_test_price_entry(1, Some(2000.0)),
                make_test_price_entry(999, Some(100.0)), // Not in catalog
            ],
            "2026-02-01T10:00:00+0100",
        );

        let result = insert_price_history(&mut conn, &guide, &catalog).unwrap();
        assert_eq!(result.inserted, 1);
        assert_eq!(result.no_product, 1);
    }

    #[test]
    fn get_latest_price_date_returns_none_when_empty() {
        let conn = test_db();
        let date = get_latest_price_date(&conn).unwrap();
        assert!(date.is_none());
    }

    #[test]
    fn get_latest_price_date_returns_latest() {
        let mut conn = test_db();

        let catalog = ProductCatalog::from_entries(vec![make_test_product(1, "Black Lotus")]);
        upsert_products(&mut conn, &catalog).unwrap();

        // Insert for multiple days
        let guide1 = PriceGuide::from_entries(
            vec![make_test_price_entry(1, Some(2000.0))],
            "2026-01-15T10:00:00+0100",
        );
        insert_price_history(&mut conn, &guide1, &catalog).unwrap();

        let guide2 = PriceGuide::from_entries(
            vec![make_test_price_entry(1, Some(2100.0))],
            "2026-02-01T10:00:00+0100",
        );
        insert_price_history(&mut conn, &guide2, &catalog).unwrap();

        let date = get_latest_price_date(&conn).unwrap();
        assert_eq!(date, Some("2026-02-01".to_string()));
    }

    #[test]
    fn get_product_count_returns_correct_count() {
        let mut conn = test_db();

        assert_eq!(get_product_count(&conn).unwrap(), 0);

        let catalog = ProductCatalog::from_entries(vec![
            make_test_product(1, "Card 1"),
            make_test_product(2, "Card 2"),
            make_test_product(3, "Card 3"),
        ]);
        upsert_products(&mut conn, &catalog).unwrap();

        assert_eq!(get_product_count(&conn).unwrap(), 3);
    }

    #[test]
    fn price_history_handles_null_prices() {
        let mut conn = test_db();

        let catalog = ProductCatalog::from_entries(vec![make_test_product(1, "Black Lotus")]);
        upsert_products(&mut conn, &catalog).unwrap();

        // Price entry with all nulls
        let guide = PriceGuide::from_entries(
            vec![make_test_price_entry(1, None)],
            "2026-02-01T10:00:00+0100",
        );

        let result = insert_price_history(&mut conn, &guide, &catalog).unwrap();
        assert_eq!(result.inserted, 1);

        // Verify null is stored correctly
        let trend: Option<f64> = conn
            .query_row(
                "SELECT trend FROM price_history WHERE id_product = ?1",
                params![1],
                |row| row.get(0),
            )
            .unwrap();
        assert!(trend.is_none());
    }
}
