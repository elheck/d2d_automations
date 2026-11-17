use super::*;
use crate::db::Db;
use crate::password;
use crate::permissions::PermissionSet;
use crate::users;
use check_stock_core::inventory_db::InventoryDb;

/// The connection comes first so it closes before the folder is removed.
fn conn_with_users(names: &[&str]) -> (rusqlite::Connection, Vec<i64>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    drop(Db::open(&InventoryDb::in_dir(dir.path()), 1).unwrap());
    let conn = crate::db::open_writer(&InventoryDb::in_dir(dir.path())).unwrap();
    let hash = password::hash("a long enough password").unwrap();
    let ids = names
        .iter()
        .map(|n| {
            users::create(
                &conn,
                n,
                n,
                &hash,
                &PermissionSet::default(),
                chrono::Utc::now(),
            )
            .unwrap()
        })
        .collect();
    (conn, ids, dir)
}

#[test]
fn the_digest_starts_at_the_previous_visit_day_per_user() {
    let (conn, ids, _dir) = conn_with_users(&["anna", "ben"]);
    let (anna, ben) = (ids[0], ids[1]);
    assert_eq!(record_visit(&conn, anna, "2026-01-01").unwrap(), None);
    assert_eq!(record_visit(&conn, anna, "2026-01-01").unwrap(), None);
    assert_eq!(
        record_visit(&conn, anna, "2026-01-05").unwrap().as_deref(),
        Some("2026-01-01")
    );
    // Again the same day: same baseline.
    assert_eq!(
        record_visit(&conn, anna, "2026-01-05").unwrap().as_deref(),
        Some("2026-01-01")
    );
    // Ben has his own days.
    assert_eq!(record_visit(&conn, ben, "2026-01-05").unwrap(), None);
    assert_eq!(
        record_visit(&conn, anna, "2026-01-09").unwrap().as_deref(),
        Some("2026-01-05")
    );
}

#[test]
fn visits_go_with_the_user() {
    let (conn, ids, _dir) = conn_with_users(&["anna"]);
    record_visit(&conn, ids[0], "2026-01-01").unwrap();
    conn.execute("DELETE FROM srv_users WHERE id = ?1", [ids[0]])
        .unwrap();
    let left: i64 = conn
        .query_row("SELECT COUNT(*) FROM srv_user_visits", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
}
