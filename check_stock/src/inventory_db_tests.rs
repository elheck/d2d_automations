//! Tests for inventory_db.

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
