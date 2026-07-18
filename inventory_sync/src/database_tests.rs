//! Tests for database.

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

#[test]
fn upsert_expansion_name_stores_and_ignores_duplicates() {
    let conn = test_db();

    upsert_expansion_name(&conn, 1, "Alpha").unwrap();

    let name: String = conn
        .query_row(
            "SELECT name FROM expansion_names WHERE id_expansion = ?1",
            params![1],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(name, "Alpha");

    // INSERT OR IGNORE: second insert with different name should be ignored
    upsert_expansion_name(&conn, 1, "Beta").unwrap();

    let name: String = conn
        .query_row(
            "SELECT name FROM expansion_names WHERE id_expansion = ?1",
            params![1],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(name, "Alpha");
}

#[test]
fn get_id_expansion_for_product_returns_correct_value() {
    let mut conn = test_db();

    let catalog = ProductCatalog::from_entries(vec![make_test_product(42, "Lightning Bolt")]);
    upsert_products(&mut conn, &catalog).unwrap();

    // make_test_product sets id_expansion = 1
    let result = get_id_expansion_for_product(&conn, 42).unwrap();
    assert_eq!(result, Some(1));

    let missing = get_id_expansion_for_product(&conn, 999).unwrap();
    assert!(missing.is_none());
}

#[test]
fn search_products_by_name_includes_expansion_name_when_known() {
    let mut conn = test_db();

    let catalog = ProductCatalog::from_entries(vec![make_test_product(1, "Black Lotus")]);
    upsert_products(&mut conn, &catalog).unwrap();

    // No expansion name stored yet — should be None
    let results = search_products_by_name(&conn, "Black Lotus", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].expansion_name.is_none());

    // Store expansion name
    upsert_expansion_name(&conn, 1, "Alpha").unwrap();

    // Now the expansion name should be joined in
    let results = search_products_by_name(&conn, "Black Lotus", 10).unwrap();
    assert_eq!(results[0].expansion_name.as_deref(), Some("Alpha"));
}

#[test]
fn get_price_history_returns_all_when_no_filter() {
    let mut conn = test_db();
    let catalog = ProductCatalog::from_entries(vec![make_test_product(1, "Black Lotus")]);
    upsert_products(&mut conn, &catalog).unwrap();

    for (date, price) in [
        ("2026-01-01T10:00:00+0100", 100.0),
        ("2026-02-01T10:00:00+0100", 110.0),
        ("2026-03-01T10:00:00+0100", 120.0),
    ] {
        let guide = PriceGuide::from_entries(vec![make_test_price_entry(1, Some(price))], date);
        insert_price_history(&mut conn, &guide, &catalog).unwrap();
    }

    let history = get_price_history(&conn, 1, None).unwrap();
    assert_eq!(history.len(), 3);
}

#[test]
fn get_price_history_filters_by_since_date() {
    let mut conn = test_db();
    let catalog = ProductCatalog::from_entries(vec![make_test_product(1, "Black Lotus")]);
    upsert_products(&mut conn, &catalog).unwrap();

    for (date, price) in [
        ("2026-01-01T10:00:00+0100", 100.0),
        ("2026-02-01T10:00:00+0100", 110.0),
        ("2026-03-01T10:00:00+0100", 120.0),
    ] {
        let guide = PriceGuide::from_entries(vec![make_test_price_entry(1, Some(price))], date);
        insert_price_history(&mut conn, &guide, &catalog).unwrap();
    }

    let history = get_price_history(&conn, 1, Some("2026-02-01")).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].price_date, "2026-02-01");
    assert_eq!(history[1].price_date, "2026-03-01");
}

#[test]
fn get_price_history_returns_empty_when_since_date_is_future() {
    let mut conn = test_db();
    let catalog = ProductCatalog::from_entries(vec![make_test_product(1, "Black Lotus")]);
    upsert_products(&mut conn, &catalog).unwrap();

    let guide = PriceGuide::from_entries(
        vec![make_test_price_entry(1, Some(100.0))],
        "2026-01-01T10:00:00+0100",
    );
    insert_price_history(&mut conn, &guide, &catalog).unwrap();

    let history = get_price_history(&conn, 1, Some("2030-01-01")).unwrap();
    assert!(history.is_empty());
}

#[test]
fn get_price_snapshots_bulk_picks_row_on_or_before_date() {
    let mut conn = test_db();
    let catalog = ProductCatalog::from_entries(vec![make_test_product(1, "Black Lotus")]);
    upsert_products(&mut conn, &catalog).unwrap();

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

    let dates = vec![
        "2026-01-20".to_string(), // between the two rows → falls back to 01-15
        "2026-02-05".to_string(), // after both → latest row
    ];
    let snapshots = get_price_snapshots_bulk(&conn, &[1], &dates).unwrap();
    assert_eq!(snapshots.len(), 2);

    assert_eq!(snapshots[0].requested_date, "2026-01-20");
    assert_eq!(snapshots[0].price_date, "2026-01-15");
    assert_eq!(snapshots[0].trend, Some(2000.0));

    assert_eq!(snapshots[1].requested_date, "2026-02-05");
    assert_eq!(snapshots[1].price_date, "2026-02-01");
    assert_eq!(snapshots[1].trend, Some(2100.0));
}

#[test]
fn get_price_snapshots_bulk_omits_missing_pairs() {
    let mut conn = test_db();
    let catalog = ProductCatalog::from_entries(vec![make_test_product(1, "Black Lotus")]);
    upsert_products(&mut conn, &catalog).unwrap();
    let guide = PriceGuide::from_entries(
        vec![make_test_price_entry(1, Some(2000.0))],
        "2026-01-15T10:00:00+0100",
    );
    insert_price_history(&mut conn, &guide, &catalog).unwrap();

    // Date before any data + a product with no history at all.
    let dates = vec!["2026-01-01".to_string()];
    let snapshots = get_price_snapshots_bulk(&conn, &[1, 999], &dates).unwrap();
    assert!(snapshots.is_empty());

    let snapshots = get_price_snapshots_bulk(&conn, &[999], &["2026-02-01".to_string()]).unwrap();
    assert!(snapshots.is_empty());
}
