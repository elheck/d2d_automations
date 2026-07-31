//! Tests for inventory_db.

use super::*;

/// Category used by the bulk of these tests. Category scoping was added later;
/// wrapping the category-taking functions here keeps the pre-existing tests
/// expressing what they were written to check (single-category behaviour)
/// rather than restating "Magic" on every line. Cross-category behaviour has
/// its own dedicated tests further down.
const CAT: &str = DEFAULT_CATEGORY;

fn sync_inventory_conn(conn: &mut Connection, cards: &[Card], today: &str) -> DbResult<SyncStats> {
    super::sync_inventory_conn(conn, cards, CAT, today)
}

fn preview_sync_conn(conn: &Connection, cards: &[Card]) -> DbResult<SyncPreview> {
    super::preview_sync_conn(conn, cards, CAT)
}

fn get_db_stats_conn(conn: &Connection, today: &str) -> DbResult<DbStats> {
    super::get_db_stats_conn(conn, CAT, today)
}

fn get_in_stock_cards_conn(conn: &Connection) -> DbResult<Vec<InStockCard>> {
    super::get_in_stock_cards_conn(conn, CAT)
}

fn get_sold_events_conn(conn: &Connection) -> DbResult<Vec<SoldEvent>> {
    super::get_sold_events_conn(conn, CAT)
}

fn get_restock_candidates_conn(conn: &Connection) -> DbResult<Vec<RestockCandidate>> {
    super::get_restock_candidates_conn(conn, Some(CAT))
}

fn discard_cards_conn(conn: &mut Connection, discards: &[(Card, i64)]) -> DbResult<DiscardStats> {
    super::discard_cards_conn(conn, discards, CAT)
}

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

    // v1 migrates through v2 to v4, so the current (category-scoped) index is
    // the one that must exist at the end.
    let index_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master \
             WHERE type='index' AND name='idx_inventory_article_key_v4'",
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

    // The v3 chain continues into v4, so the category-scoped index is what
    // must exist afterwards; the v3 index must be gone.
    let v4_idx: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master \
             WHERE type='index' AND name='idx_inventory_article_key_v4'",
            [],
            |_| Ok(true),
        )
        .optional()
        .unwrap()
        .unwrap_or(false);
    assert!(v4_idx, "v4 index must exist after v3→v2→v4 migration");

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

// ==================== preview_sync / import safety ====================

#[test]
fn preview_on_empty_db_is_all_new() {
    let conn = test_conn();
    let cards = vec![make_card("1", "Alpha", "3"), make_card("2", "Beta", "2")];
    let p = preview_sync_conn(&conn, &cards).unwrap();
    assert_eq!(p.new_variants, 2);
    assert_eq!(p.updated_variants, 0);
    assert_eq!(p.zeroed_variants, 0);
    assert_eq!(p.copies_sold, 0);
    assert_eq!(p.copies_before, 0);
    assert_eq!(p.copies_after, 5);
    assert!(!p.is_suspicious());
}

#[test]
fn preview_detects_drops_zeroes_and_price_changes() {
    let mut conn = test_conn();
    let cards = vec![
        make_card("1", "Alpha", "5"),
        make_card("2", "Beta", "4"),
        make_card("3", "Gamma", "1"),
    ];
    sync_inventory_conn(&mut conn, &cards, "2026-01-01").unwrap();

    // Next CSV: Alpha dropped to 2 copies with a new price, Beta unchanged,
    // Gamma vanished, Delta is new.
    let mut alpha = make_card("1", "Alpha", "2");
    alpha.price = "2,50".to_string(); // comma decimal must parse too
    let next = vec![
        alpha,
        make_card("2", "Beta", "4"),
        make_card("4", "Delta", "1"),
    ];
    let p = preview_sync_conn(&conn, &next).unwrap();

    assert_eq!(p.new_variants, 1);
    assert_eq!(p.updated_variants, 2);
    assert_eq!(p.zeroed_variants, 1);
    // 3 copies of Alpha dropped + 1 vanished Gamma copy.
    assert_eq!(p.copies_sold, 4);
    assert_eq!(p.price_changes, 1);
    assert_eq!(p.copies_before, 10);
    assert_eq!(p.copies_after, 7);
    // Nothing was written by the preview itself.
    assert_eq!(count_rows(&conn), 3);
}

#[test]
fn preview_suspicious_only_on_large_relative_drop() {
    // Small inventory: even a total wipe is not guarded (fresh DBs churn).
    let small = SyncPreview {
        copies_before: 50,
        copies_sold: 50,
        ..SyncPreview::default()
    };
    assert!(!small.is_suspicious());

    // Large inventory, >50% recorded as sold → suspicious.
    let bad = SyncPreview {
        copies_before: 200,
        copies_sold: 101,
        ..SyncPreview::default()
    };
    assert!(bad.is_suspicious());

    // Exactly half is still allowed.
    let half = SyncPreview {
        copies_before: 200,
        copies_sold: 100,
        ..SyncPreview::default()
    };
    assert!(!half.is_suspicious());
}

#[test]
fn backup_creates_dated_snapshot_and_prunes() {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = test_conn();
    sync_inventory_conn(&mut conn, &[make_card("1", "Alpha", "3")], "2026-01-01").unwrap();

    backup_db_at(&conn, dir.path(), "2026-01-01", 2).unwrap();
    let first = dir.path().join("inventory-2026-01-01.db.bak");
    assert!(first.exists());
    // The snapshot is a valid database containing the synced row.
    let snap = Connection::open(&first).unwrap();
    let n: i64 = snap
        .query_row("SELECT COUNT(*) FROM inventory_cards", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);

    // Same day again: idempotent, no error, still one file.
    backup_db_at(&conn, dir.path(), "2026-01-01", 2).unwrap();

    // Two more days: oldest backup is pruned (keep = 2).
    backup_db_at(&conn, dir.path(), "2026-01-02", 2).unwrap();
    backup_db_at(&conn, dir.path(), "2026-01-03", 2).unwrap();
    assert!(!first.exists());
    assert!(dir.path().join("inventory-2026-01-02.db.bak").exists());
    assert!(dir.path().join("inventory-2026-01-03.db.bak").exists());
}

// ==================== visit digest ====================

#[test]
fn digest_first_visit_has_no_baseline() {
    let conn = test_conn();
    let d = visit_digest_conn(&conn, "2026-01-01").unwrap();
    assert_eq!(d.since, None);
    assert_eq!(d.sold_copies, 0);
    assert_eq!(d.last_sync, None);
    // The visit was recorded.
    assert_eq!(
        get_meta(&conn, "last_visit").unwrap().as_deref(),
        Some("2026-01-01")
    );
}

#[test]
fn digest_counts_changes_since_previous_visit_day() {
    let mut conn = test_conn();
    // Day 1: visit + initial stock.
    visit_digest_conn(&conn, "2026-01-01").unwrap();
    sync_inventory_conn(&mut conn, &[make_card("1", "Alpha", "5")], "2026-01-01").unwrap();

    // Day 3: 2 Alpha copies sold, Beta newly listed.
    sync_inventory_conn(
        &mut conn,
        &[make_card("1", "Alpha", "3"), make_card("2", "Beta", "1")],
        "2026-01-03",
    )
    .unwrap();

    let d = visit_digest_conn(&conn, "2026-01-03").unwrap();
    assert_eq!(d.since.as_deref(), Some("2026-01-01"));
    assert_eq!(d.sold_copies, 2);
    assert!((d.sold_revenue - 2.0).abs() < 0.001); // 2 copies × €1.00
    assert_eq!(d.new_listings, 1); // Beta
    assert_eq!(d.last_sync.as_deref(), Some("2026-01-03"));

    // Restarting the app on the same day keeps the same baseline.
    let again = visit_digest_conn(&conn, "2026-01-03").unwrap();
    assert_eq!(again.since.as_deref(), Some("2026-01-01"));
    assert_eq!(again.sold_copies, 2);
}

#[test]
fn digest_counts_restock_candidates() {
    let mut conn = test_conn();
    sync_inventory_conn(&mut conn, &[make_card("1", "Alpha", "2")], "2026-01-01").unwrap();
    // Alpha sells out entirely.
    sync_inventory_conn(&mut conn, &[], "2026-01-02").unwrap();
    let d = visit_digest_conn(&conn, "2026-01-02").unwrap();
    assert_eq!(d.restock_candidates, 1);
}

#[test]
fn restock_candidates_carry_source_lot() {
    let mut conn = test_conn();
    let mut with_lot = make_card("1", "Alpha", "2");
    with_lot.location = Some("A-0-0-31-L2-R".to_string());
    let mut no_lot = make_card("2", "Beta", "1");
    no_lot.location = Some("A-0-0-32".to_string());
    sync_inventory_conn(&mut conn, &[with_lot, no_lot], "2026-01-01").unwrap();
    // Both sell out entirely.
    sync_inventory_conn(&mut conn, &[], "2026-01-02").unwrap();

    let mut cands = get_restock_candidates_conn(&conn).unwrap();
    cands.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(cands.len(), 2);
    assert_eq!(cands[0].lot.as_deref(), Some("L2"));
    assert_eq!(cands[1].lot, None);
}

// ── Category scoping ───────────────────────────────────────────────────────
//
// Cardmarket exports one inventory report per category, each complete only for
// itself. These tests pin the property the whole feature exists for: a sync of
// one category must never disturb another.

/// Builds a Generic-category row (accessory: no set, no collector number, no rarity).
fn make_generic(id: &str, name: &str, qty: &str) -> Card {
    let mut card = make_card(id, name, qty);
    card.set = String::new();
    card.set_code = String::new();
    card.cn = String::new();
    card.rarity = String::new();
    card
}

/// Reads (quantity, sold_quantity) for a variant in a specific category.
fn qty_sold_in(conn: &Connection, category: &str, id: &str) -> Option<(i64, i64)> {
    conn.query_row(
        "SELECT quantity, sold_quantity FROM inventory_cards
         WHERE category = ?1 AND cardmarket_id = ?2",
        params![category, id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .unwrap()
}

const GENERIC: &str = "Generic";

#[test]
fn generic_sync_does_not_zero_magic_stock() {
    // The core regression: loading the Generic export must leave every Magic
    // row exactly as it was, rather than zeroing it as "vanished from the CSV".
    let mut conn = test_conn();
    let magic = vec![
        make_card("1", "Lightning Bolt", "4"),
        make_card("2", "Counterspell", "3"),
    ];
    super::sync_inventory_conn(&mut conn, &magic, DEFAULT_CATEGORY, "2026-01-01").unwrap();

    let generic = vec![make_generic("716833", "TCG Guru Sleeves", "14")];
    super::sync_inventory_conn(&mut conn, &generic, GENERIC, "2026-01-02").unwrap();

    assert_eq!(qty_sold_in(&conn, DEFAULT_CATEGORY, "1"), Some((4, 0)));
    assert_eq!(qty_sold_in(&conn, DEFAULT_CATEGORY, "2"), Some((3, 0)));
    assert_eq!(qty_sold_in(&conn, GENERIC, "716833"), Some((14, 0)));
}

#[test]
fn generic_sync_records_no_phantom_sales_for_magic() {
    // Zeroing is only half the damage — it also inflates sold_quantity and the
    // sold_events log, which would corrupt revenue and restock reporting.
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[make_card("1", "Lightning Bolt", "4")],
        DEFAULT_CATEGORY,
        "2026-01-01",
    )
    .unwrap();

    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("716833", "Sleeves", "5")],
        GENERIC,
        "2026-01-02",
    )
    .unwrap();

    let magic_events = super::get_sold_events_conn(&conn, DEFAULT_CATEGORY).unwrap();
    assert!(
        magic_events.is_empty(),
        "Generic sync must not log Magic sales, got {magic_events:?}"
    );
    assert_eq!(qty_sold_in(&conn, DEFAULT_CATEGORY, "1"), Some((4, 0)));
}

#[test]
fn magic_sync_does_not_zero_generic_stock() {
    // Symmetry: the protection must hold in both directions.
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("716833", "Sleeves", "14")],
        GENERIC,
        "2026-01-01",
    )
    .unwrap();
    super::sync_inventory_conn(
        &mut conn,
        &[make_card("1", "Lightning Bolt", "4")],
        DEFAULT_CATEGORY,
        "2026-01-02",
    )
    .unwrap();

    assert_eq!(qty_sold_in(&conn, GENERIC, "716833"), Some((14, 0)));
    assert_eq!(qty_sold_in(&conn, DEFAULT_CATEGORY, "1"), Some((4, 0)));
}

#[test]
fn same_product_id_in_two_categories_stays_separate() {
    // Cardmarket product IDs are namespaced per category, so the same numeric
    // ID can be a card in one and an accessory in the other. They must occupy
    // two independent rows, not collide onto one.
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[make_card("716833", "Some Card", "4")],
        DEFAULT_CATEGORY,
        "2026-01-01",
    )
    .unwrap();
    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("716833", "Some Sleeves", "9")],
        GENERIC,
        "2026-01-01",
    )
    .unwrap();

    assert_eq!(
        count_rows(&conn),
        2,
        "IDs must not collide across categories"
    );
    assert_eq!(qty_sold_in(&conn, DEFAULT_CATEGORY, "716833"), Some((4, 0)));
    assert_eq!(qty_sold_in(&conn, GENERIC, "716833"), Some((9, 0)));

    let magic_name: String = conn
        .query_row(
            "SELECT name FROM inventory_cards WHERE category = ?1 AND cardmarket_id = '716833'",
            params![DEFAULT_CATEGORY],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(magic_name, "Some Card");
}

#[test]
fn zeroing_still_works_within_a_category() {
    // Scoping must not disable the real behaviour: a variant that genuinely
    // vanishes from its own category's CSV is still zeroed and booked as sold.
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[
            make_generic("1", "Sleeves A", "5"),
            make_generic("2", "Sleeves B", "3"),
        ],
        GENERIC,
        "2026-01-01",
    )
    .unwrap();
    // "Sleeves B" sold out and left the export.
    let stats = super::sync_inventory_conn(
        &mut conn,
        &[make_generic("1", "Sleeves A", "5")],
        GENERIC,
        "2026-01-02",
    )
    .unwrap();

    assert_eq!(stats.zeroed, 1);
    assert_eq!(qty_sold_in(&conn, GENERIC, "2"), Some((0, 3)));
    assert_eq!(qty_sold_in(&conn, GENERIC, "1"), Some((5, 0)));
}

#[test]
fn sold_events_are_tagged_with_their_category() {
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("1", "Sleeves", "5")],
        GENERIC,
        "2026-01-01",
    )
    .unwrap();
    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("1", "Sleeves", "2")],
        GENERIC,
        "2026-01-02",
    )
    .unwrap();

    let generic_events = super::get_sold_events_conn(&conn, GENERIC).unwrap();
    assert_eq!(generic_events.len(), 1);
    assert_eq!(generic_events[0].copies, 3);

    // The same events must not leak into the Magic view.
    assert!(super::get_sold_events_conn(&conn, DEFAULT_CATEGORY)
        .unwrap()
        .is_empty());
}

#[test]
fn preview_counts_only_its_own_category() {
    // The suspicious-change guard divides copies_sold by copies_before. If
    // copies_before counted other categories, a legitimate Generic wipe would
    // look harmless — and a real Generic problem would hide behind Magic's bulk.
    let mut conn = test_conn();
    let magic: Vec<Card> = (0..50)
        .map(|i| make_card(&format!("m{i}"), "Card", "10"))
        .collect();
    super::sync_inventory_conn(&mut conn, &magic, DEFAULT_CATEGORY, "2026-01-01").unwrap();
    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("g1", "Sleeves", "4")],
        GENERIC,
        "2026-01-01",
    )
    .unwrap();

    let preview = super::preview_sync_conn(&conn, &[], GENERIC).unwrap();
    assert_eq!(preview.copies_before, 4, "must see only Generic copies");
    assert_eq!(preview.copies_sold, 4);
    assert_eq!(preview.zeroed_variants, 1);
}

#[test]
fn guard_does_not_fire_for_a_generic_sync_against_a_large_magic_db() {
    // A Generic export arriving at a big Magic DB is a completely normal event
    // and must sync cleanly rather than tripping the safety check.
    let mut conn = test_conn();
    let magic: Vec<Card> = (0..50)
        .map(|i| make_card(&format!("m{i}"), "Card", "10"))
        .collect();
    super::sync_inventory_conn(&mut conn, &magic, DEFAULT_CATEGORY, "2026-01-01").unwrap();

    let preview =
        super::preview_sync_conn(&conn, &[make_generic("g1", "Sleeves", "14")], GENERIC).unwrap();
    assert!(
        !preview.is_suspicious(),
        "first Generic import must not be flagged: {preview:?}"
    );
    assert_eq!(preview.zeroed_variants, 0);
    assert_eq!(preview.new_variants, 1);
}

#[test]
fn guard_still_fires_within_a_category() {
    // Scoping must not weaken the guard: a truncated Generic CSV that would
    // wipe out the Generic inventory is still caught.
    let mut conn = test_conn();
    let generic: Vec<Card> = (0..30)
        .map(|i| make_generic(&format!("g{i}"), "Sleeves", "10"))
        .collect();
    super::sync_inventory_conn(&mut conn, &generic, GENERIC, "2026-01-01").unwrap();

    let preview = super::preview_sync_conn(&conn, &[], GENERIC).unwrap();
    assert!(
        preview.is_suspicious(),
        "wiping a category's stock must still be flagged: {preview:?}"
    );
}

#[test]
fn stats_are_scoped_to_their_category() {
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[make_card("1", "Lightning Bolt", "4")],
        DEFAULT_CATEGORY,
        "2026-01-01",
    )
    .unwrap();
    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("716833", "Sleeves", "10")],
        GENERIC,
        "2026-01-01",
    )
    .unwrap();

    let magic_stats = super::get_db_stats_conn(&conn, DEFAULT_CATEGORY, "2026-01-02").unwrap();
    assert_eq!(magic_stats.total_articles, 1);
    assert_eq!(magic_stats.total_copies, 4);

    let generic_stats = super::get_db_stats_conn(&conn, GENERIC, "2026-01-02").unwrap();
    assert_eq!(generic_stats.total_articles, 1);
    assert_eq!(generic_stats.total_copies, 10);
}

#[test]
fn in_stock_cards_are_scoped_to_their_category() {
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[make_card("1", "Lightning Bolt", "4")],
        DEFAULT_CATEGORY,
        "2026-01-01",
    )
    .unwrap();
    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("716833", "Sleeves", "10")],
        GENERIC,
        "2026-01-01",
    )
    .unwrap();

    let magic = super::get_in_stock_cards_conn(&conn, DEFAULT_CATEGORY).unwrap();
    assert_eq!(magic.len(), 1);
    assert_eq!(magic[0].name, "Lightning Bolt");

    let generic = super::get_in_stock_cards_conn(&conn, GENERIC).unwrap();
    assert_eq!(generic.len(), 1);
    assert_eq!(generic[0].name, "Sleeves");
}

#[test]
fn restock_candidates_are_scoped_to_their_category() {
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[make_card("1", "Lightning Bolt", "4")],
        DEFAULT_CATEGORY,
        "2026-01-01",
    )
    .unwrap();
    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("716833", "Sleeves", "10")],
        GENERIC,
        "2026-01-01",
    )
    .unwrap();
    // Both sell out, each in its own export.
    super::sync_inventory_conn(&mut conn, &[], DEFAULT_CATEGORY, "2026-01-02").unwrap();
    super::sync_inventory_conn(&mut conn, &[], GENERIC, "2026-01-02").unwrap();

    let magic = super::get_restock_candidates_conn(&conn, Some(DEFAULT_CATEGORY)).unwrap();
    assert_eq!(magic.len(), 1);
    assert_eq!(magic[0].name, "Lightning Bolt");
    assert_eq!(magic[0].sold_copies, 4);

    let generic = super::get_restock_candidates_conn(&conn, Some(GENERIC)).unwrap();
    assert_eq!(generic.len(), 1);
    assert_eq!(generic[0].name, "Sleeves");
    assert_eq!(generic[0].sold_copies, 10);
}

#[test]
fn restock_revenue_does_not_mix_events_across_categories() {
    // The sold_events join must match on category too; otherwise a same-ID
    // variant in another category would contribute its revenue here.
    let mut conn = test_conn();
    let mut magic = make_card("716833", "Some Card", "2");
    magic.price = "10.00".to_string();
    let mut generic = make_generic("716833", "Sleeves", "2");
    generic.price = "1.00".to_string();

    super::sync_inventory_conn(&mut conn, &[magic], DEFAULT_CATEGORY, "2026-01-01").unwrap();
    super::sync_inventory_conn(&mut conn, &[generic], GENERIC, "2026-01-01").unwrap();
    super::sync_inventory_conn(&mut conn, &[], DEFAULT_CATEGORY, "2026-01-02").unwrap();
    super::sync_inventory_conn(&mut conn, &[], GENERIC, "2026-01-02").unwrap();

    let magic_cands = super::get_restock_candidates_conn(&conn, Some(DEFAULT_CATEGORY)).unwrap();
    assert_eq!(magic_cands.len(), 1);
    assert!(
        (magic_cands[0].realized_revenue - 20.0).abs() < 0.001,
        "expected 2 × €10, got {}",
        magic_cands[0].realized_revenue
    );

    let generic_cands = super::get_restock_candidates_conn(&conn, Some(GENERIC)).unwrap();
    assert_eq!(generic_cands.len(), 1);
    assert!(
        (generic_cands[0].realized_revenue - 2.0).abs() < 0.001,
        "expected 2 × €1, got {}",
        generic_cands[0].realized_revenue
    );
}

#[test]
fn discard_only_touches_the_named_category() {
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[make_card("716833", "Some Card", "5")],
        DEFAULT_CATEGORY,
        "2026-01-01",
    )
    .unwrap();
    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("716833", "Sleeves", "5")],
        GENERIC,
        "2026-01-01",
    )
    .unwrap();

    let stats = super::discard_cards_conn(
        &mut conn,
        &[(make_generic("716833", "Sleeves", "5"), 2)],
        GENERIC,
    )
    .unwrap();

    assert_eq!(stats.copies_discarded, 2);
    assert_eq!(qty_sold_in(&conn, GENERIC, "716833"), Some((3, 0)));
    assert_eq!(
        qty_sold_in(&conn, DEFAULT_CATEGORY, "716833"),
        Some((5, 0)),
        "the same ID in another category must be untouched"
    );
}

#[test]
fn lot_breakdown_combines_cards_and_accessories_in_one_lot() {
    // A lot is a purchase, and its recorded cost covers everything bought —
    // sleeves included. Both categories must therefore contribute to the same
    // lot row, or margin and payback would be measured against a cost that
    // covers stock the row doesn't count.
    let mut conn = test_conn();
    let mut magic = make_card("1", "Lightning Bolt", "2");
    magic.location = Some("A-0-0-1-L22-R".to_string());
    magic.price = "10.00".to_string();
    let mut generic = make_generic("2", "Sleeves", "3");
    generic.location = Some("A-0-0-0-L22-R".to_string());
    generic.price = "1.00".to_string();

    super::sync_inventory_conn(&mut conn, &[magic], DEFAULT_CATEGORY, "2026-01-01").unwrap();
    super::sync_inventory_conn(&mut conn, &[generic], GENERIC, "2026-01-01").unwrap();

    let lots = super::lot_breakdown_from(&conn).unwrap();
    assert_eq!(lots.len(), 1, "one purchase must be one lot row");
    assert_eq!(lots[0].lot, "L22");
    assert_eq!(lots[0].in_stock_listings, 2);
    assert_eq!(lots[0].in_stock_copies, 5, "2 cards + 3 sleeves");
    assert!(
        (lots[0].in_stock_value - 23.0).abs() < 0.001,
        "2×€10 + 3×€1, got {}",
        lots[0].in_stock_value
    );
}

#[test]
fn lot_revenue_and_payback_include_accessory_sales() {
    // The payback column compares one recorded cost against the lot's revenue.
    // Accessories sold out of the lot must count towards recouping it.
    let mut conn = test_conn();
    let mut magic = make_card("1", "Lightning Bolt", "2");
    magic.location = Some("A-0-0-1-L22-R".to_string());
    magic.price = "10.00".to_string();
    let mut generic = make_generic("2", "Sleeves", "4");
    generic.location = Some("A-0-0-0-L22-R".to_string());
    generic.price = "5.00".to_string();

    super::sync_inventory_conn(&mut conn, &[magic.clone()], DEFAULT_CATEGORY, "2026-01-01")
        .unwrap();
    super::sync_inventory_conn(&mut conn, &[generic.clone()], GENERIC, "2026-01-01").unwrap();

    // One card and two sleeve packs sell: €10 + €10 = €20 revenue.
    magic.quantity = "1".to_string();
    generic.quantity = "2".to_string();
    super::sync_inventory_conn(&mut conn, &[magic], DEFAULT_CATEGORY, "2026-01-02").unwrap();
    super::sync_inventory_conn(&mut conn, &[generic], GENERIC, "2026-01-02").unwrap();

    set_lot_cost_conn(&conn, "L22", 15.0, "2026-01-01").unwrap();

    let lots = super::lot_breakdown_from(&conn).unwrap();
    assert_eq!(lots.len(), 1);
    assert_eq!(lots[0].sold_copies, 3, "1 card + 2 sleeve packs");
    assert!(
        (lots[0].sold_revenue - 20.0).abs() < 0.001,
        "1×€10 + 2×€5, got {}",
        lots[0].sold_revenue
    );
    assert_eq!(
        lots[0].is_recouped(),
        Some(true),
        "€20 revenue against €15 cost is recouped"
    );
    assert_eq!(lots[0].cost_to_recoup(), Some(0.0));
}

#[test]
fn lot_breakdown_appears_identically_regardless_of_stats_category() {
    // The lot table is whole-business, so viewing Magic stats and Generic stats
    // must not change the lot figures.
    let mut conn = test_conn();
    let mut magic = make_card("1", "Lightning Bolt", "2");
    magic.location = Some("A-0-0-1-L22-R".to_string());
    let mut generic = make_generic("2", "Sleeves", "3");
    generic.location = Some("A-0-0-0-L22-R".to_string());

    super::sync_inventory_conn(&mut conn, &[magic], DEFAULT_CATEGORY, "2026-01-01").unwrap();
    super::sync_inventory_conn(&mut conn, &[generic], GENERIC, "2026-01-01").unwrap();

    let from_magic = super::get_db_stats_conn(&conn, DEFAULT_CATEGORY, "2026-01-02").unwrap();
    let from_generic = super::get_db_stats_conn(&conn, GENERIC, "2026-01-02").unwrap();

    assert_eq!(from_magic.lot_breakdown.len(), 1);
    assert_eq!(from_magic.lot_breakdown[0].in_stock_copies, 5);
    assert_eq!(
        from_magic.lot_breakdown[0].in_stock_copies, from_generic.lot_breakdown[0].in_stock_copies,
        "lot figures must not depend on which category's stats are shown"
    );
}

#[test]
fn categories_are_listed_alphabetically() {
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("1", "Sleeves", "1")],
        GENERIC,
        "2026-01-01",
    )
    .unwrap();
    super::sync_inventory_conn(
        &mut conn,
        &[make_card("2", "Bolt", "1")],
        DEFAULT_CATEGORY,
        "2026-01-01",
    )
    .unwrap();

    assert_eq!(
        super::get_categories_conn(&conn).unwrap(),
        vec!["Generic".to_string(), "Magic".to_string()]
    );
}

#[test]
fn snapshot_spans_all_categories() {
    // Snapshots are the whole-business position, so both categories count —
    // and syncing either one recomputes the combined figure correctly.
    let mut conn = test_conn();
    let mut magic = make_card("1", "Bolt", "2");
    magic.price = "10.00".to_string();
    let mut generic = make_generic("2", "Sleeves", "3");
    generic.price = "1.00".to_string();

    super::sync_inventory_conn(&mut conn, &[magic], DEFAULT_CATEGORY, "2026-01-01").unwrap();
    super::sync_inventory_conn(&mut conn, &[generic], GENERIC, "2026-01-01").unwrap();

    let snaps = read_snapshots_conn(&conn).unwrap();
    let last = snaps.last().unwrap();
    assert_eq!(last.in_stock_copies, 5, "2 cards + 3 sleeves");
    assert!(
        (last.in_stock_value - 23.0).abs() < 0.001,
        "2×€10 + 3×€1, got {}",
        last.in_stock_value
    );
}

#[test]
fn migration_v2_to_v4_backfills_magic_and_preserves_history() {
    // A real user's DB is at v2 with Magic-only data. The migration must add the
    // category without losing quantities, sold history or first_synced_at.
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE inventory_cards (
            cardmarket_id TEXT NOT NULL, quantity INTEGER NOT NULL,
            name TEXT NOT NULL, set_name TEXT NOT NULL, set_code TEXT NOT NULL,
            cn TEXT NOT NULL, condition TEXT NOT NULL, language TEXT NOT NULL,
            is_foil TEXT NOT NULL, is_playset TEXT, is_signed TEXT NOT NULL,
            price TEXT NOT NULL, comment TEXT NOT NULL, location TEXT,
            name_de TEXT NOT NULL, name_es TEXT NOT NULL, name_fr TEXT NOT NULL,
            name_it TEXT NOT NULL, rarity TEXT NOT NULL, listed_at TEXT NOT NULL,
            first_synced_at TEXT NOT NULL, last_synced_at TEXT NOT NULL,
            sold_quantity INTEGER NOT NULL DEFAULT 0
        );
        CREATE UNIQUE INDEX idx_inventory_article_key
            ON inventory_cards (cardmarket_id, condition, language, is_foil, is_signed);
        INSERT INTO inventory_cards VALUES
            ('1', 4, 'Lightning Bolt', 'Alpha', 'LEA', '1', 'NM', 'English',
             '', NULL, '', '2.00', '', 'A-0-0-1-L2-R', '', '', '', '', 'Common',
             '2024-01-01', '2025-06-01', '2026-01-01', 7);",
    )
    .unwrap();

    init_schema(&conn).unwrap();

    let (category, qty, sold, first): (String, i64, i64, String) = conn
        .query_row(
            "SELECT category, quantity, sold_quantity, first_synced_at FROM inventory_cards",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(category, DEFAULT_CATEGORY, "existing rows are Magic");
    assert_eq!(qty, 4);
    assert_eq!(sold, 7, "sold history must survive");
    assert_eq!(first, "2025-06-01", "first_synced_at must survive");

    // The old 5-field index must be gone so IDs can repeat across categories.
    let old_idx: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master \
             WHERE type='index' AND name='idx_inventory_article_key'",
            [],
            |_| Ok(true),
        )
        .optional()
        .unwrap()
        .unwrap_or(false);
    assert!(!old_idx, "the un-scoped unique index must be replaced");

    // Idempotent: opening again changes nothing.
    init_schema(&conn).unwrap();
    assert_eq!(count_rows(&conn), 1);
}

#[test]
fn migrated_db_accepts_a_generic_sync_without_touching_magic() {
    // End-to-end for the actual upgrade path: an existing v2 Magic database is
    // migrated, then the new Generic export is loaded into it.
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE inventory_cards (
            cardmarket_id TEXT NOT NULL, quantity INTEGER NOT NULL,
            name TEXT NOT NULL, set_name TEXT NOT NULL, set_code TEXT NOT NULL,
            cn TEXT NOT NULL, condition TEXT NOT NULL, language TEXT NOT NULL,
            is_foil TEXT NOT NULL, is_playset TEXT, is_signed TEXT NOT NULL,
            price TEXT NOT NULL, comment TEXT NOT NULL, location TEXT,
            name_de TEXT NOT NULL, name_es TEXT NOT NULL, name_fr TEXT NOT NULL,
            name_it TEXT NOT NULL, rarity TEXT NOT NULL, listed_at TEXT NOT NULL,
            first_synced_at TEXT NOT NULL, last_synced_at TEXT NOT NULL,
            sold_quantity INTEGER NOT NULL DEFAULT 0
        );
        CREATE UNIQUE INDEX idx_inventory_article_key
            ON inventory_cards (cardmarket_id, condition, language, is_foil, is_signed);
        INSERT INTO inventory_cards VALUES
            ('716833', 4, 'Lightning Bolt', 'Alpha', 'LEA', '1', 'NM', 'English',
             '', NULL, '', '2.00', '', NULL, '', '', '', '', 'Common',
             '2024-01-01', '2025-06-01', '2026-01-01', 0);",
    )
    .unwrap();
    init_schema(&conn).unwrap();

    // The Generic export happens to reuse the same product ID.
    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("716833", "TCG Guru Sleeves", "14")],
        GENERIC,
        "2026-02-01",
    )
    .unwrap();

    assert_eq!(
        qty_sold_in(&conn, DEFAULT_CATEGORY, "716833"),
        Some((4, 0)),
        "pre-existing Magic stock must be untouched"
    );
    assert_eq!(qty_sold_in(&conn, GENERIC, "716833"), Some((14, 0)));
    assert_eq!(count_rows(&conn), 2);
}

#[test]
fn sold_events_migration_adds_category_to_existing_rows() {
    // sold_events is created on every open, so an old DB has the table without
    // the column. Existing events belong to Magic.
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE sold_events (
            date TEXT NOT NULL, cardmarket_id TEXT NOT NULL,
            condition TEXT NOT NULL, language TEXT NOT NULL,
            is_foil TEXT NOT NULL, is_signed TEXT NOT NULL,
            copies INTEGER NOT NULL, price REAL NOT NULL
        );
        INSERT INTO sold_events VALUES
            ('2026-01-01', '1', 'NM', 'English', '', '', 3, 2.50);",
    )
    .unwrap();

    init_schema(&conn).unwrap();

    let events = super::get_sold_events_conn(&conn, DEFAULT_CATEGORY).unwrap();
    assert_eq!(events.len(), 1, "pre-existing events must be Magic");
    assert_eq!(events[0].copies, 3);
    assert!(super::get_sold_events_conn(&conn, GENERIC)
        .unwrap()
        .is_empty());
}

#[test]
fn unknown_category_reads_return_nothing_rather_than_everything() {
    // Defensive: a typo'd or unseen category must never fall back to "all rows".
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[make_card("1", "Lightning Bolt", "4")],
        DEFAULT_CATEGORY,
        "2026-01-01",
    )
    .unwrap();

    assert!(super::get_in_stock_cards_conn(&conn, "Nonexistent")
        .unwrap()
        .is_empty());
    let stats = super::get_db_stats_conn(&conn, "Nonexistent", "2026-01-02").unwrap();
    assert_eq!(stats.total_articles, 0);
    assert_eq!(stats.total_copies, 0);
}

#[test]
fn syncing_an_empty_generic_export_leaves_magic_alone() {
    // Edge case: an empty (or fully sold-out) Generic export must still be a
    // no-op for Magic, even though "no rows" is exactly the shape that would
    // have triggered the mass-zeroing bug.
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[make_card("1", "Lightning Bolt", "4")],
        DEFAULT_CATEGORY,
        "2026-01-01",
    )
    .unwrap();

    let stats = super::sync_inventory_conn(&mut conn, &[], GENERIC, "2026-01-02").unwrap();
    assert_eq!(stats.zeroed, 0);
    assert_eq!(stats.upserted, 0);
    assert_eq!(qty_sold_in(&conn, DEFAULT_CATEGORY, "1"), Some((4, 0)));
}

#[test]
fn restock_candidates_span_all_categories_when_unscoped() {
    // The restock report is a buying decision: fast-moving sleeves belong on
    // the buy list next to the cards.
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[make_card("1", "Lightning Bolt", "4")],
        DEFAULT_CATEGORY,
        "2026-01-01",
    )
    .unwrap();
    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("2", "Sleeves", "10")],
        GENERIC,
        "2026-01-01",
    )
    .unwrap();
    super::sync_inventory_conn(&mut conn, &[], DEFAULT_CATEGORY, "2026-01-02").unwrap();
    super::sync_inventory_conn(&mut conn, &[], GENERIC, "2026-01-02").unwrap();

    let mut all = super::get_restock_candidates_conn(&conn, None).unwrap();
    all.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(all.len(), 2, "both categories must appear");
    assert_eq!(all[0].name, "Lightning Bolt");
    assert_eq!(all[1].name, "Sleeves");
    assert_eq!(all[1].sold_copies, 10);
}

#[test]
fn unscoped_restock_candidates_keep_their_source_lot() {
    // Accessories are shelved in lots too; the buy list must show which
    // purchase a sold-out sleeve pack came from.
    let mut conn = test_conn();
    let mut generic = make_generic("716833", "TCG Guru Sleeves", "14");
    generic.location = Some("A-0-0-0-L22-R".to_string());
    super::sync_inventory_conn(&mut conn, &[generic], GENERIC, "2026-01-01").unwrap();
    super::sync_inventory_conn(&mut conn, &[], GENERIC, "2026-01-02").unwrap();

    let all = super::get_restock_candidates_conn(&conn, None).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].lot.as_deref(), Some("L22"));
    assert_eq!(all[0].sold_copies, 14);
}

#[test]
fn unscoped_restock_does_not_merge_same_id_across_categories() {
    // Two different products sharing an ID must stay two rows even when the
    // report spans categories.
    let mut conn = test_conn();
    super::sync_inventory_conn(
        &mut conn,
        &[make_card("716833", "Some Card", "2")],
        DEFAULT_CATEGORY,
        "2026-01-01",
    )
    .unwrap();
    super::sync_inventory_conn(
        &mut conn,
        &[make_generic("716833", "Some Sleeves", "3")],
        GENERIC,
        "2026-01-01",
    )
    .unwrap();
    super::sync_inventory_conn(&mut conn, &[], DEFAULT_CATEGORY, "2026-01-02").unwrap();
    super::sync_inventory_conn(&mut conn, &[], GENERIC, "2026-01-02").unwrap();

    let mut all = super::get_restock_candidates_conn(&conn, None).unwrap();
    all.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].name, "Some Card");
    assert_eq!(all[0].sold_copies, 2);
    assert_eq!(all[1].name, "Some Sleeves");
    assert_eq!(all[1].sold_copies, 3);
}

#[test]
fn lot_revenue_follows_a_variant_that_moved_lots() {
    // When a variant's location changes, the upsert overwrites `location`, so
    // its whole history (including copies sold while it sat in the old lot)
    // moves to the new lot. Revenue is derived from the current location, not
    // from where the copies sold — this is the documented behaviour, and the
    // lot table must reflect the move rather than keeping a stale split.
    let mut conn = test_conn();
    let mut before = make_generic("716833", "Sleeves", "10");
    before.location = Some("A-0-0-0-L22-R".to_string());
    before.price = "2.00".to_string();
    super::sync_inventory_conn(&mut conn, &[before], GENERIC, "2026-01-01").unwrap();

    // Four sell, and the shelf label is corrected to L23 in the same export.
    let mut after = make_generic("716833", "Sleeves", "6");
    after.location = Some("A-0-0-0-L23-R".to_string());
    after.price = "2.00".to_string();
    super::sync_inventory_conn(&mut conn, &[after], GENERIC, "2026-01-02").unwrap();

    let lots = super::lot_breakdown_from(&conn).unwrap();
    assert_eq!(lots.len(), 1, "the variant lives in exactly one lot");
    assert_eq!(lots[0].lot, "L23", "the lot table must follow the move");
    assert_eq!(lots[0].in_stock_copies, 6);
    assert_eq!(lots[0].sold_copies, 4);
    assert!(
        (lots[0].sold_revenue - 8.0).abs() < 0.001,
        "4 × €2, got {}",
        lots[0].sold_revenue
    );
}

#[test]
fn forced_sync_writes_the_same_lot_figures_as_a_normal_one() {
    // A sync confirmed past the guard must land identically to an unguarded
    // one — the guard only gates *whether* the write happens, never what it
    // writes. (What made the lot line look unchanged in the UI was the cached
    // stats not reloading after the forced write, not the write itself.)
    let mut guarded = test_conn();
    let mut forced = test_conn();

    let mut initial = make_generic("716833", "Sleeves", "10");
    initial.location = Some("A-0-0-0-L22-R".to_string());
    initial.price = "2.00".to_string();
    for conn in [&mut guarded, &mut forced] {
        super::sync_inventory_conn(conn, &[initial.clone()], GENERIC, "2026-01-01").unwrap();
    }

    let mut moved = make_generic("716833", "Sleeves", "1");
    moved.location = Some("A-0-0-0-L23-R".to_string());
    moved.price = "2.00".to_string();

    // The drop from 10 to 1 is exactly the shape the guard flags.
    let preview =
        super::preview_sync_conn(&guarded, std::slice::from_ref(&moved), GENERIC).unwrap();
    assert_eq!(preview.copies_sold, 9);

    super::sync_inventory_conn(&mut guarded, &[moved.clone()], GENERIC, "2026-01-02").unwrap();
    super::sync_inventory_conn(&mut forced, &[moved], GENERIC, "2026-01-02").unwrap();

    let a = super::lot_breakdown_from(&guarded).unwrap();
    let b = super::lot_breakdown_from(&forced).unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].lot, "L23");
    assert_eq!(a[0].sold_copies, b[0].sold_copies);
    assert!((a[0].sold_revenue - b[0].sold_revenue).abs() < 0.001);
    assert!((a[0].sold_revenue - 18.0).abs() < 0.001, "9 × €2");
}
