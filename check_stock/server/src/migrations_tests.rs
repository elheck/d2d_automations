use super::*;
use check_stock_core::inventory_db::InventoryDb;

fn tables(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap();
    stmt.query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn fresh_database_gets_every_migration_once() {
    let mut conn = Connection::open_in_memory().unwrap();
    assert_eq!(current_version(&conn).unwrap(), 0);
    let applied = run(&mut conn).unwrap();
    assert_eq!(applied, (1..=LATEST).collect::<Vec<_>>());
    assert_eq!(current_version(&conn).unwrap(), LATEST);
    assert!(run(&mut conn).unwrap().is_empty(), "second run is a no-op");
}

#[test]
fn every_server_table_has_the_prefix() {
    let mut conn = Connection::open_in_memory().unwrap();
    run(&mut conn).unwrap();
    let names = tables(&conn);
    assert!(!names.is_empty());
    for name in names {
        assert!(
            name.starts_with(TABLE_PREFIX),
            "{name} lacks the {TABLE_PREFIX} prefix"
        );
    }
}

#[test]
fn core_tables_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = InventoryDb::in_dir(dir.path()).open().unwrap();
    let before = tables(&conn);
    assert!(
        before.iter().all(|t| !t.starts_with(TABLE_PREFIX)),
        "core never uses the prefix"
    );
    run(&mut conn).unwrap();
    let after: Vec<String> = tables(&conn)
        .into_iter()
        .filter(|t| !t.starts_with(TABLE_PREFIX))
        .collect();
    assert_eq!(after, before);
    // Core still recognises its schema afterwards.
    drop(conn);
    assert!(InventoryDb::in_dir(dir.path())
        .get_in_stock_cards("Magic")
        .unwrap()
        .is_empty());
}

#[test]
fn a_newer_schema_is_refused() {
    let mut conn = Connection::open_in_memory().unwrap();
    run(&mut conn).unwrap();
    conn.execute(
        "INSERT INTO srv_migrations (version, applied_at) VALUES (?1, 'later')",
        [LATEST + 1],
    )
    .unwrap();
    match run(&mut conn) {
        Err(MigrationError::NewerSchema { found, supported }) => {
            assert_eq!(found, LATEST + 1);
            assert_eq!(supported, LATEST);
        }
        other => panic!("expected NewerSchema, got {other:?}"),
    }
}

#[test]
fn a_failing_migration_leaves_nothing_behind() {
    // The migration SQL and its version row commit together: simulate a
    // half-applied state by creating a conflicting table first.
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE srv_meta (x INTEGER);")
        .unwrap();
    assert!(run(&mut conn).is_err());
    assert_eq!(current_version(&conn).unwrap(), 0);
}

#[test]
fn meta_round_trip() {
    let mut conn = Connection::open_in_memory().unwrap();
    run(&mut conn).unwrap();
    assert_eq!(get_meta(&conn, "k").unwrap(), None);
    set_meta(&conn, "k", "one").unwrap();
    set_meta(&conn, "k", "two").unwrap();
    assert_eq!(get_meta(&conn, "k").unwrap().as_deref(), Some("two"));
}

#[test]
fn timestamps_are_rfc3339_utc() {
    let now = now_utc();
    assert!(now.ends_with('Z'), "{now}");
    assert!(chrono::DateTime::parse_from_rfc3339(&now).is_ok(), "{now}");
}
