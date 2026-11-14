use super::*;
use chrono::TimeZone;

fn db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    crate::migrations::run(&mut conn).unwrap();
    conn
}

fn t(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 14, 12, 0, second).unwrap()
}

#[test]
fn entries_record_actor_action_and_details() {
    let conn = db();
    record(
        &conn,
        t(1),
        Actor::User {
            id: 7,
            username: "anna",
        },
        action::USER_CREATED,
        Some("ben"),
        serde_json::json!({ "permissions": ["search.use"] }),
    )
    .unwrap();
    record(
        &conn,
        t(2),
        Actor::Cli,
        action::DESKTOP_IMPORTED,
        None,
        serde_json::json!({}),
    )
    .unwrap();
    let entries = list(&conn, None, 10).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].actor, "cli", "newest first");
    assert_eq!(entries[1].actor, "anna");
    assert_eq!(entries[1].action, "user.created");
    assert_eq!(entries[1].target.as_deref(), Some("ben"));
    assert_eq!(entries[1].at, "2026-11-14T12:00:01Z");
    assert_eq!(entries[1].details["permissions"][0], "search.use");
    let user_id: Option<i64> = conn
        .query_row(
            "SELECT actor_user_id FROM srv_audit_log WHERE actor = 'anna'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(user_id, Some(7));
}

#[test]
fn pages_go_backwards_in_time() {
    let conn = db();
    for i in 0..5 {
        record(
            &conn,
            t(i),
            Actor::Anonymous,
            action::LOGIN_FAILED,
            None,
            serde_json::json!({ "n": i }),
        )
        .unwrap();
    }
    let first = list(&conn, None, 2).unwrap();
    assert_eq!(
        first
            .iter()
            .map(|e| e.details["n"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [4, 3]
    );
    let second = list(&conn, Some(first[1].id), 2).unwrap();
    assert_eq!(
        second
            .iter()
            .map(|e| e.details["n"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [2, 1]
    );
}

#[test]
fn page_size_is_capped() {
    let conn = db();
    for i in 0..(MAX_PAGE + 5) {
        record(
            &conn,
            t(0),
            Actor::Cli,
            "x",
            None,
            serde_json::json!({ "i": i }),
        )
        .unwrap();
    }
    assert_eq!(list(&conn, None, 10_000).unwrap().len(), MAX_PAGE as usize);
    assert_eq!(list(&conn, None, 0).unwrap().len(), 1, "at least one");
}

#[test]
fn entries_survive_deleting_the_user() {
    let dir = tempfile::tempdir().unwrap();
    let conn = crate::db::open_writer(&check_stock_core::inventory_db::InventoryDb::in_dir(
        dir.path(),
    ))
    .unwrap();
    let id = crate::users::create(&conn, "anna", "Anna", "$h", &Default::default(), t(0)).unwrap();
    record(
        &conn,
        t(1),
        Actor::User {
            id,
            username: "anna",
        },
        action::LOGIN,
        None,
        serde_json::json!({}),
    )
    .unwrap();
    crate::users::delete(&conn, id).unwrap();
    let entries = list(&conn, None, 10).unwrap();
    assert_eq!(entries[0].actor, "anna");
}
