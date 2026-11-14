use super::*;
use crate::db::open_writer;
use check_stock_core::inventory_db::InventoryDb;
use chrono::TimeZone;

fn db() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_writer(&InventoryDb::in_dir(dir.path())).unwrap();
    (dir, conn)
}

fn t(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 14, 12, minute, 0).unwrap()
}

fn perms(keys: &[&str]) -> PermissionSet {
    PermissionSet::from_keys(keys).unwrap()
}

fn add(conn: &Connection, name: &str, keys: &[&str]) -> i64 {
    create(conn, name, name, "$hash", &perms(keys), t(0)).unwrap()
}

#[test]
fn usernames_are_normalized_and_checked() {
    assert_eq!(normalize_username("  Anna.B ").unwrap(), "anna.b");
    assert_eq!(normalize_username("helper-2").unwrap(), "helper-2");
    for bad in [
        "ab",
        "",
        "a b c",
        "änna",
        "_anna",
        ".anna",
        "anna@shop",
        &"x".repeat(33),
    ] {
        assert!(
            normalize_username(bad).is_err(),
            "{bad:?} should be rejected"
        );
    }
}

#[test]
fn display_names_are_checked() {
    assert_eq!(
        normalize_display_name("  Anna Müller ").unwrap(),
        "Anna Müller"
    );
    assert!(normalize_display_name("   ").is_err());
    assert!(normalize_display_name("a\u{7}b").is_err());
    assert!(normalize_display_name(&"x".repeat(65)).is_err());
}

#[test]
fn create_and_find() {
    let (_dir, conn) = db();
    let id = create(
        &conn,
        "anna",
        "Anna",
        "$hash",
        &perms(&["search.use"]),
        t(0),
    )
    .unwrap();
    let anna = find_by_username(&conn, "ANNA")
        .unwrap()
        .expect("case-insensitive");
    assert_eq!(anna.id, id);
    assert_eq!(anna.display_name, "Anna");
    assert!(anna.can(Permission::SearchUse));
    assert!(!anna.can(Permission::UsersManage));
    assert!(!anna.disabled);
    assert_eq!(anna.created_at, "2026-11-14T12:00:00Z");
    assert_eq!(find_by_id(&conn, id).unwrap().unwrap(), anna);
    assert_eq!(find_by_id(&conn, id + 100).unwrap(), None);
    assert_eq!(count(&conn).unwrap(), 1);
}

#[test]
fn usernames_are_unique_regardless_of_case() {
    let (_dir, conn) = db();
    add(&conn, "anna", &[]);
    assert!(matches!(
        create(&conn, "anna", "Other", "$h", &PermissionSet::new(), t(0)),
        Err(StoreError::UsernameTaken)
    ));
    // Even if a caller forgot to normalize, the column collation catches it.
    assert!(create(&conn, "ANNA", "Other", "$h", &PermissionSet::new(), t(0)).is_err());
}

#[test]
fn list_is_sorted_and_carries_permissions() {
    let (_dir, conn) = db();
    add(&conn, "zoe", &["listing.use"]);
    add(&conn, "anna", &["users.manage"]);
    let all = list(&conn).unwrap();
    let names: Vec<&str> = all.iter().map(|u| u.username.as_str()).collect();
    assert_eq!(names, ["anna", "zoe"]);
    assert!(all[1].can(Permission::ListingUse));
}

#[test]
fn permissions_are_replaced_as_a_whole() {
    let (_dir, conn) = db();
    let id = add(&conn, "anna", &["search.use", "listing.use"]);
    set_permissions(&conn, id, &perms(&["audit.view"])).unwrap();
    assert_eq!(permissions_of(&conn, id).unwrap().keys(), ["audit.view"]);
}

#[test]
fn unknown_stored_permissions_are_ignored() {
    let (_dir, conn) = db();
    let id = add(&conn, "anna", &["search.use"]);
    conn.execute(
        "INSERT INTO srv_user_permissions (user_id, permission) VALUES (?1, 'future.thing')",
        [id],
    )
    .unwrap();
    assert_eq!(permissions_of(&conn, id).unwrap().keys(), ["search.use"]);
}

#[test]
fn deleting_a_user_removes_permissions_and_sessions() {
    let (_dir, conn) = db();
    let id = add(&conn, "anna", &["search.use"]);
    let token = crate::sessions::SessionToken::generate().unwrap();
    crate::sessions::create(&conn, &token, id, None, t(0)).unwrap();
    delete(&conn, id).unwrap();
    let left: i64 = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM srv_user_permissions) + (SELECT COUNT(*) FROM srv_sessions)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(left, 0);
    assert!(matches!(delete(&conn, id), Err(StoreError::NotFound)));
}

#[test]
fn changes_to_missing_users_are_reported() {
    let (_dir, conn) = db();
    assert!(matches!(
        set_display_name(&conn, 9, "x"),
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        set_disabled(&conn, 9, true),
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        set_password(&conn, 9, "$h", t(0)),
        Err(StoreError::NotFound)
    ));
    assert!(matches!(unlock(&conn, 9), Err(StoreError::NotFound)));
    assert!(matches!(
        record_failed_login(&conn, 9, t(0)),
        Err(StoreError::NotFound)
    ));
}

#[test]
fn too_many_failures_lock_the_account_for_a_while() {
    let (_dir, conn) = db();
    let id = add(&conn, "anna", &[]);
    for _ in 1..MAX_FAILED_LOGINS {
        assert!(!record_failed_login(&conn, id, t(1)).unwrap());
    }
    assert!(
        record_failed_login(&conn, id, t(1)).unwrap(),
        "locked on the 5th"
    );
    let anna = find_by_id(&conn, id).unwrap().unwrap();
    assert!(anna.is_locked(t(2)));
    assert!(anna.is_locked(t(15)));
    assert!(!anna.is_locked(t(16)), "15 minutes later it is open again");
    assert_eq!(anna.failed_logins, 0, "the counter starts over");
}

#[test]
fn a_successful_login_resets_the_counter() {
    let (_dir, conn) = db();
    let id = add(&conn, "anna", &[]);
    for _ in 1..MAX_FAILED_LOGINS {
        record_failed_login(&conn, id, t(1)).unwrap();
    }
    record_successful_login(&conn, id, t(2)).unwrap();
    assert!(!record_failed_login(&conn, id, t(3)).unwrap());
    let anna = find_by_id(&conn, id).unwrap().unwrap();
    assert_eq!(anna.failed_logins, 1);
    assert_eq!(anna.last_login_at.as_deref(), Some("2026-11-14T12:02:00Z"));
}

#[test]
fn a_new_password_unlocks() {
    let (_dir, conn) = db();
    let id = add(&conn, "anna", &[]);
    for _ in 0..MAX_FAILED_LOGINS {
        record_failed_login(&conn, id, t(1)).unwrap();
    }
    set_password(&conn, id, "$new", t(2)).unwrap();
    let anna = find_by_id(&conn, id).unwrap().unwrap();
    assert!(!anna.is_locked(t(3)));
    assert_eq!(anna.password_hash, "$new");
    assert_eq!(anna.password_changed_at, "2026-11-14T12:02:00Z");
}

#[test]
fn unlock_lifts_the_lockout() {
    let (_dir, conn) = db();
    let id = add(&conn, "anna", &[]);
    for _ in 0..MAX_FAILED_LOGINS {
        record_failed_login(&conn, id, t(1)).unwrap();
    }
    unlock(&conn, id).unwrap();
    assert!(!find_by_id(&conn, id).unwrap().unwrap().is_locked(t(2)));
}

#[test]
fn admin_counting_ignores_disabled_users() {
    let (_dir, conn) = db();
    assert_eq!(enabled_admin_count(&conn).unwrap(), 0);
    let a = add(&conn, "anna", &["users.manage"]);
    add(&conn, "ben", &["search.use"]);
    assert_eq!(enabled_admin_count(&conn).unwrap(), 1);
    assert!(ensure_an_admin_remains(&conn).is_ok());
    set_disabled(&conn, a, true).unwrap();
    assert_eq!(enabled_admin_count(&conn).unwrap(), 0);
    assert!(matches!(
        ensure_an_admin_remains(&conn),
        Err(StoreError::LastAdmin)
    ));
}

#[test]
fn the_last_admin_check_rolls_a_change_back() {
    let (_dir, mut conn) = db();
    let a = add(&conn, "anna", &["users.manage"]);
    let result = crate::api::in_transaction(&mut conn, |tx| {
        set_permissions(tx, a, &PermissionSet::new())?;
        ensure_an_admin_remains(tx)
    })
    .unwrap();
    assert!(matches!(result, Err(StoreError::LastAdmin)));
    assert!(find_by_id(&conn, a)
        .unwrap()
        .unwrap()
        .can(Permission::UsersManage));
}
