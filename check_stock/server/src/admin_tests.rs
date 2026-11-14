use super::*;
use crate::permissions::Permission;
use std::io::Cursor;

const PW: &str = "a long admin password";

fn conn(dir: &Path) -> rusqlite::Connection {
    open_writer(&inventory_db_in(dir)).unwrap()
}

#[test]
fn create_admin_grants_every_permission_and_logs_it() {
    let dir = tempfile::tempdir().unwrap();
    let outcome = create_admin(
        dir.path(),
        "Aron",
        Some("Aron H."),
        false,
        |_| Ok(PW.into()),
    )
    .unwrap();
    assert_eq!(
        outcome,
        CreateAdminOutcome::Created {
            username: "aron".into()
        }
    );

    let conn = conn(dir.path());
    let aron = users::find_by_username(&conn, "aron").unwrap().unwrap();
    assert_eq!(aron.display_name, "Aron H.");
    for &p in Permission::ALL {
        assert!(aron.can(p), "{p:?}");
    }
    assert!(password::verify(PW, &aron.password_hash));
    let log = audit::list(&conn, None, 10).unwrap();
    assert_eq!(
        (log[0].actor.as_str(), log[0].action.as_str()),
        ("cli", "user.created")
    );
    assert_eq!(log[0].target.as_deref(), Some("aron"));
}

#[test]
fn the_display_name_defaults_to_the_login_name() {
    let dir = tempfile::tempdir().unwrap();
    create_admin(dir.path(), "aron", None, false, |_| Ok(PW.into())).unwrap();
    let aron = users::find_by_username(&conn(dir.path()), "aron")
        .unwrap()
        .unwrap();
    assert_eq!(aron.display_name, "aron");
}

#[test]
fn if_none_skips_without_asking_for_a_password() {
    let dir = tempfile::tempdir().unwrap();
    create_admin(dir.path(), "aron", None, false, |_| Ok(PW.into())).unwrap();
    let outcome = create_admin(
        dir.path(),
        "second",
        None,
        true,
        |_| -> std::io::Result<String> { panic!("must not ask") },
    )
    .unwrap();
    assert_eq!(outcome, CreateAdminOutcome::SkippedUsersExist);
    assert_eq!(users::count(&conn(dir.path())).unwrap(), 1);
}

#[test]
fn invalid_input_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        create_admin(dir.path(), "x", None, false, |_| Ok(PW.into())),
        Err(AdminError::Invalid(_))
    ));
    assert!(matches!(
        create_admin(dir.path(), "aron", None, false, |_| Ok("short".into())),
        Err(AdminError::Invalid(_))
    ));
    create_admin(dir.path(), "aron", None, false, |_| Ok(PW.into())).unwrap();
    assert!(matches!(
        create_admin(dir.path(), "ARON", None, false, |_| Ok(PW.into())),
        Err(AdminError::Store(StoreError::UsernameTaken))
    ));
    assert_eq!(users::count(&conn(dir.path())).unwrap(), 1);
}

#[test]
fn reset_password_unlocks_and_logs_out() {
    let dir = tempfile::tempdir().unwrap();
    create_admin(dir.path(), "aron", None, false, |_| Ok(PW.into())).unwrap();
    {
        let conn = conn(dir.path());
        let id = users::find_by_username(&conn, "aron").unwrap().unwrap().id;
        for _ in 0..users::MAX_FAILED_LOGINS {
            users::record_failed_login(&conn, id, chrono::Utc::now()).unwrap();
        }
        sessions::create(
            &conn,
            &sessions::SessionToken::generate().unwrap(),
            id,
            None,
            chrono::Utc::now(),
        )
        .unwrap();
    }
    reset_password(dir.path(), "Aron", |_| Ok("the new admin password".into())).unwrap();

    let conn = conn(dir.path());
    let aron = users::find_by_username(&conn, "aron").unwrap().unwrap();
    assert!(!aron.is_locked(chrono::Utc::now()));
    assert!(password::verify(
        "the new admin password",
        &aron.password_hash
    ));
    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM srv_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sessions, 0);
    assert_eq!(
        audit::list(&conn, None, 1).unwrap()[0].action,
        "user.password_reset"
    );
}

#[test]
fn reset_password_for_an_unknown_user_fails() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        reset_password(dir.path(), "nobody", |_| Ok(PW.into())),
        Err(AdminError::Store(StoreError::NotFound))
    ));
}

#[test]
fn admin_commands_refuse_a_folder_in_use() {
    let dir = tempfile::tempdir().unwrap();
    let _server = DataDirLock::acquire(dir.path()).unwrap();
    assert!(matches!(
        create_admin(dir.path(), "aron", None, false, |_| Ok(PW.into())),
        Err(AdminError::Lock(LockError::InUse(_)))
    ));
    assert!(matches!(
        reset_password(dir.path(), "aron", |_| Ok(PW.into())),
        Err(AdminError::Lock(LockError::InUse(_)))
    ));
}

#[test]
fn passwords_from_stdin_lose_only_the_line_break() {
    assert_eq!(
        read_password_line(&mut Cursor::new("secret phrase here\n")).unwrap(),
        "secret phrase here"
    );
    assert_eq!(
        read_password_line(&mut Cursor::new("windows line\r\n")).unwrap(),
        "windows line"
    );
    assert_eq!(
        read_password_line(&mut Cursor::new("  spaces kept  \n")).unwrap(),
        "  spaces kept  "
    );
    assert_eq!(
        read_password_line(&mut Cursor::new("no newline")).unwrap(),
        "no newline"
    );
    assert_eq!(read_password_line(&mut Cursor::new("")).unwrap(), "");
}
