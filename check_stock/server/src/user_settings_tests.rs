use super::*;
use crate::db::open_writer;
use crate::permissions::PermissionSet;
use crate::users;
use check_stock_core::inventory_db::InventoryDb;
use chrono::TimeZone;
use serde_json::json;

fn db() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_writer(&InventoryDb::in_dir(dir.path())).unwrap();
    (dir, conn)
}

fn t(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 15, 12, minute, 0).unwrap()
}

fn value(v: serde_json::Value) -> SettingValue {
    SettingValue::new(v).unwrap()
}

fn add_user(conn: &Connection, name: &str) -> i64 {
    users::create(conn, name, name, "$hash", &PermissionSet::default(), t(0)).unwrap()
}

#[test]
fn keys_never_change() {
    // Stored in the database: renaming one loses everyone's settings.
    assert_eq!(Setting::BuyHelper.key(), "buy_helper");
    assert_eq!(Setting::PreconEv.key(), "precon_ev");
}

#[test]
fn settings_are_stored_per_user() {
    let (_dir, conn) = db();
    let anna = add_user(&conn, "anna");
    let ben = add_user(&conn, "ben");
    assert_eq!(get(&conn, anna, Setting::BuyHelper).unwrap(), None);

    assert!(put(
        &conn,
        anna,
        Setting::BuyHelper,
        &value(json!({"percent": 60})),
        t(1)
    )
    .unwrap());
    assert!(put(
        &conn,
        ben,
        Setting::BuyHelper,
        &value(json!({"percent": 50})),
        t(1)
    )
    .unwrap());
    assert_eq!(
        get(&conn, anna, Setting::BuyHelper).unwrap(),
        Some(json!({"percent": 60}))
    );
    assert_eq!(
        get(&conn, ben, Setting::BuyHelper).unwrap(),
        Some(json!({"percent": 50}))
    );

    assert!(put(
        &conn,
        anna,
        Setting::BuyHelper,
        &value(json!({"percent": 70})),
        t(2)
    )
    .unwrap());
    assert_eq!(
        get(&conn, anna, Setting::BuyHelper).unwrap(),
        Some(json!({"percent": 70}))
    );
    let updated: String = conn
        .query_row(
            "SELECT updated_at FROM srv_user_settings WHERE user_id = ?1",
            [anna],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(updated, format_utc(t(2)));
}

#[test]
fn an_unchanged_value_is_not_written() {
    let (_dir, conn) = db();
    let anna = add_user(&conn, "anna");
    assert!(put(
        &conn,
        anna,
        Setting::BuyHelper,
        &value(json!({"a": 1})),
        t(1)
    )
    .unwrap());
    assert!(!put(
        &conn,
        anna,
        Setting::BuyHelper,
        &value(json!({"a": 1})),
        t(5)
    )
    .unwrap());
    let updated: String = conn
        .query_row("SELECT updated_at FROM srv_user_settings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(updated, format_utc(t(1)));
}

#[test]
fn oversized_values_are_refused() {
    let huge = json!({ "x": "a".repeat(MAX_VALUE_BYTES) });
    assert_eq!(SettingValue::new(huge), Err(TooLarge));
    let fits = json!({ "x": "a".repeat(MAX_VALUE_BYTES - 20) });
    assert_eq!(SettingValue::new(fits.clone()).unwrap().value(), &fits);
}

#[test]
fn settings_go_with_the_user() {
    let (_dir, conn) = db();
    let anna = add_user(&conn, "anna");
    put(
        &conn,
        anna,
        Setting::BuyHelper,
        &value(json!({"a": 1})),
        t(1),
    )
    .unwrap();
    users::delete(&conn, anna).unwrap();
    let left: i64 = conn
        .query_row("SELECT COUNT(*) FROM srv_user_settings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn only_json_can_be_stored() {
    let (_dir, conn) = db();
    let anna = add_user(&conn, "anna");
    let raw = conn.execute(
        "INSERT INTO srv_user_settings (user_id, key, value, updated_at) VALUES (?1, 'x', 'not json', 'now')",
        [anna],
    );
    assert!(raw.is_err());
}

#[test]
fn values_come_back_as_stored_for_the_screen_to_check() {
    let (_dir, conn) = db();
    let anna = add_user(&conn, "anna");
    // Valid JSON, but not what the screen expects: the screen validates it.
    conn.execute(
        "INSERT INTO srv_user_settings (user_id, key, value, updated_at) VALUES (?1, 'buy_helper', '1', 'now')",
        [anna],
    )
    .unwrap();
    assert_eq!(
        get(&conn, anna, Setting::BuyHelper).unwrap(),
        Some(json!(1))
    );
}
