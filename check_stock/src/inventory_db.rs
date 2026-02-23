//! Local SQLite database for inventory sync.
//!
//! Every time a Cardmarket inventory CSV is loaded, the cards are synced here.
//! - Articles no longer in the CSV have their quantity set to 0 (never deleted).
//! - `first_synced_at`: set once on first insert, never changed.
//! - `last_synced_at`: updated to today on each sync, max once per day.
//! - Articles already at quantity 0 and not in the CSV are left untouched.

use crate::models::Card;
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::path::PathBuf;

/// Result type for database operations
pub type DbResult<T> = Result<T, rusqlite::Error>;

/// Statistics from a sync operation
#[derive(Debug, Default)]
pub struct SyncStats {
    /// Number of cards from the CSV that were inserted or updated
    pub upserted: usize,
    /// Number of cards no longer in the CSV that were set to quantity 0
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

/// Creates the `inventory_cards` table if it does not already exist.
fn init_schema(conn: &Connection) -> DbResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS inventory_cards (
            cardmarket_id   TEXT NOT NULL PRIMARY KEY,
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
        );",
    )
}

/// Returns today's date as `YYYY-MM-DD` using local system time.
fn today_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Syncs a slice of cards (from a freshly loaded inventory CSV) to the local DB.
///
/// - Existing cards are updated; new cards are inserted.
/// - Cards not in `cards` that have `quantity > 0` are zeroed out.
/// - Timestamps are only advanced once per day.
///
/// Errors are returned to the caller; call sites treat them as non-fatal.
pub fn sync_inventory(cards: &[Card]) -> DbResult<SyncStats> {
    let mut conn = open_db()?;
    sync_inventory_conn(&mut conn, cards, &today_date())
}

/// Inner sync that accepts an explicit connection and date — used in tests.
fn sync_inventory_conn(conn: &mut Connection, cards: &[Card], today: &str) -> DbResult<SyncStats> {
    log::debug!("Syncing {} cards to inventory DB ({})", cards.len(), today);
    let tx = conn.transaction()?;
    let mut stats = SyncStats::default();

    // Phase 1: upsert all cards from the CSV
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
            ON CONFLICT(cardmarket_id) DO UPDATE SET
                quantity        = excluded.quantity,
                name            = excluded.name,
                set_name        = excluded.set_name,
                set_code        = excluded.set_code,
                cn              = excluded.cn,
                condition       = excluded.condition,
                language        = excluded.language,
                is_foil         = excluded.is_foil,
                is_playset      = excluded.is_playset,
                is_signed       = excluded.is_signed,
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
                -- first_synced_at is intentionally excluded: preserved from the original INSERT",
        )?;

        for card in cards {
            let qty: i64 = card.quantity.trim().parse().unwrap_or(0);
            stmt.execute(params![
                card.cardmarket_id,
                qty,
                card.name,
                card.set,
                card.set_code,
                card.cn,
                card.condition,
                card.language,
                card.is_foil,
                card.is_playset,
                card.is_signed,
                card.price,
                card.comment,
                card.location,
                card.name_de,
                card.name_es,
                card.name_fr,
                card.name_it,
                card.rarity,
                card.listed_at,
                today,
            ])?;
            stats.upserted += 1;
        }
    }

    // Phase 2: zero out articles no longer in the CSV
    let csv_ids: HashSet<&str> = cards.iter().map(|c| c.cardmarket_id.as_str()).collect();

    // Collect all (id, quantity, last_synced_at) from DB
    let db_rows: Vec<(String, i64, String)> = tx
        .prepare("SELECT cardmarket_id, quantity, last_synced_at FROM inventory_cards")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<DbResult<Vec<_>>>()?;

    for (id, quantity, last_synced_at) in &db_rows {
        if !csv_ids.contains(id.as_str()) && *quantity > 0 {
            let new_date = if last_synced_at == today {
                last_synced_at.as_str()
            } else {
                today
            };
            tx.execute(
                "UPDATE inventory_cards
                 SET quantity = 0, last_synced_at = ?1
                 WHERE cardmarket_id = ?2",
                params![new_date, id],
            )?;
            stats.zeroed += 1;
        }
    }

    tx.commit()?;
    if stats.upserted > 0 || stats.zeroed > 0 {
        log::info!(
            "Inventory DB sync: {} updated, {} zeroed",
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
