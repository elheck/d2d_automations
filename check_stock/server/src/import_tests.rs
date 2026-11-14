use super::*;
use check_stock_core::models::Card;

/// A desktop-style database with `n` in-stock cards.
fn desktop_db(dir: &Path, n: usize) -> PathBuf {
    let db = InventoryDb::in_dir(dir);
    let cards: Vec<Card> = (0..n)
        .map(|i| {
            let mut card = Card::test_default();
            card.cardmarket_id = (1000 + i).to_string();
            card.name = format!("Card {i}");
            card
        })
        .collect();
    db.sync_inventory(&cards, "Magic").unwrap();
    db.path().to_path_buf()
}

fn rows(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM inventory_cards", [], |r| r.get(0))
        .unwrap()
}

fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.contains(".importing") || n.ends_with("-wal") || n.ends_with("-shm"))
        .collect()
}

#[test]
fn copies_the_desktop_database() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let source = desktop_db(desktop.path(), 3);

    let report = import_desktop(&source, data.path(), false).unwrap();
    assert_eq!(report.target, data.path().join("inventory.db"));
    assert_eq!(report.inventory_rows, 3);
    assert!(report.previous_kept_as.is_none());
    assert_eq!(rows(&report.target), 3);

    let conn = Connection::open(&report.target).unwrap();
    assert_eq!(
        migrations::current_version(&conn).unwrap(),
        migrations::LATEST
    );
    assert_eq!(
        migrations::get_meta(&conn, META_COPY_SOURCE).unwrap(),
        Some(source.canonicalize().unwrap().display().to_string())
    );
    assert_eq!(
        migrations::get_meta(&conn, META_COPY_TAKEN_AT).unwrap(),
        Some(report.taken_at.clone())
    );
    assert!(
        leftovers(data.path()).is_empty(),
        "{:?}",
        leftovers(data.path())
    );
}

#[test]
fn the_source_is_never_changed() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let source = desktop_db(desktop.path(), 5);
    let before = std::fs::read(&source).unwrap();
    let files_before = std::fs::read_dir(desktop.path()).unwrap().count();

    import_desktop(&source, data.path(), false).unwrap();

    assert_eq!(std::fs::read(&source).unwrap(), before, "byte for byte");
    assert_eq!(
        std::fs::read_dir(desktop.path()).unwrap().count(),
        files_before
    );
}

#[test]
fn an_existing_database_needs_replace() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let source = desktop_db(desktop.path(), 2);
    import_desktop(&source, data.path(), false).unwrap();

    match import_desktop(&source, data.path(), false) {
        Err(ImportError::TargetExists(path)) => assert_eq!(path, data.path().join("inventory.db")),
        other => panic!("expected TargetExists, got {other:?}"),
    }
}

#[test]
fn replace_keeps_the_previous_database() {
    let old_desktop = tempfile::tempdir().unwrap();
    let new_desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    import_desktop(&desktop_db(old_desktop.path(), 2), data.path(), false).unwrap();

    let report = import_desktop(&desktop_db(new_desktop.path(), 7), data.path(), true).unwrap();
    assert_eq!(rows(&report.target), 7);
    let kept = report.previous_kept_as.expect("previous database kept");
    assert!(kept
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("inventory.db.replaced-"));
    assert_eq!(rows(&kept), 2, "the backup holds the old data");
    assert!(
        leftovers(data.path()).is_empty(),
        "{:?}",
        leftovers(data.path())
    );
}

#[test]
fn replace_keeps_data_that_was_still_in_the_wal() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let source = desktop_db(desktop.path(), 1);
    import_desktop(&source, data.path(), false).unwrap();

    // A server wrote in WAL mode and crashed before a checkpoint. The files
    // as a crash leaves them are copied while the connection is still open
    // (a leaked open handle would block deleting the file on Windows).
    let target = data.path().join("inventory.db");
    let crashed = tempfile::tempdir().unwrap();
    {
        let conn = Connection::open(&target).unwrap();
        let _: String = conn
            .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
            .unwrap();
        conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        migrations::set_meta(&conn, "only_in_wal", "yes").unwrap();
        for name in ["inventory.db", "inventory.db-wal"] {
            std::fs::copy(data.path().join(name), crashed.path().join(name)).unwrap();
        }
    }
    // Premise: the change is only in the WAL. The database file alone
    // (copied without its WAL) does not have it yet.
    assert!(crashed.path().join("inventory.db-wal").exists());
    let main_only = tempfile::tempdir().unwrap();
    std::fs::copy(
        crashed.path().join("inventory.db"),
        main_only.path().join("inventory.db"),
    )
    .unwrap();
    let main_conn = Connection::open(main_only.path().join("inventory.db")).unwrap();
    assert_eq!(
        migrations::get_meta(&main_conn, "only_in_wal").unwrap(),
        None
    );
    drop(main_conn);

    let report = import_desktop(&source, crashed.path(), true).unwrap();
    let kept = Connection::open(report.previous_kept_as.unwrap()).unwrap();
    assert_eq!(
        migrations::get_meta(&kept, "only_in_wal")
            .unwrap()
            .as_deref(),
        Some("yes")
    );
}

#[test]
fn the_server_database_cannot_import_itself() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    import_desktop(&desktop_db(desktop.path(), 1), data.path(), false).unwrap();
    let target = data.path().join("inventory.db");
    assert!(matches!(
        import_desktop(&target, data.path(), true),
        Err(ImportError::SameFile(_))
    ));
    assert_eq!(rows(&target), 1);
}

#[test]
fn a_missing_source_is_reported() {
    let data = tempfile::tempdir().unwrap();
    let missing = data.path().join("nope.db");
    assert!(matches!(
        import_desktop(&missing, data.path(), false),
        Err(ImportError::SourceMissing(_))
    ));
}

#[test]
fn files_that_are_not_inventory_databases_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();

    let text = dir.path().join("notes.db");
    std::fs::write(
        &text,
        "this is not a database, just some text that is long enough",
    )
    .unwrap();
    let other = dir.path().join("other.db");
    Connection::open(&other)
        .unwrap()
        .execute_batch("CREATE TABLE something (x INTEGER);")
        .unwrap();

    for source in [text, other] {
        match import_desktop(&source, data.path(), false) {
            // The import reports the canonical path (macOS: /var is
            // /private/var; Windows: \\?\ prefix and long folder names).
            Err(ImportError::NotAnInventoryDb(path, _)) => {
                assert_eq!(path, source.canonicalize().unwrap())
            }
            other => panic!(
                "expected NotAnInventoryDb for {}, got {other:?}",
                source.display()
            ),
        }
    }
    assert!(!data.path().join("inventory.db").exists());
    assert!(
        leftovers(data.path()).is_empty(),
        "{:?}",
        leftovers(data.path())
    );
}

#[test]
fn a_damaged_database_never_becomes_the_server_database() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let source = desktop_db(desktop.path(), 400);
    // Overwrite a page in the middle of the file (not the header page).
    let mut bytes = std::fs::read(&source).unwrap();
    let page = 4096;
    assert!(bytes.len() > 4 * page, "test database needs several pages");
    let start = bytes.len() / page / 2 * page;
    bytes[start..start + page].fill(0xA5);
    std::fs::write(&source, bytes).unwrap();

    let result = import_desktop(&source, data.path(), false);
    assert!(
        matches!(
            result,
            Err(ImportError::Corrupt(_) | ImportError::Sqlite(_))
        ),
        "{result:?}"
    );
    assert!(!data.path().join("inventory.db").exists());
    assert!(
        leftovers(data.path()).is_empty(),
        "{:?}",
        leftovers(data.path())
    );
}

#[test]
fn no_import_while_the_folder_is_in_use() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let source = desktop_db(desktop.path(), 1);
    let _server = DataDirLock::acquire(data.path()).unwrap();
    assert!(matches!(
        import_desktop(&source, data.path(), false),
        Err(ImportError::Lock(LockError::InUse(_)))
    ));
    assert!(!data.path().join("inventory.db").exists());
}

#[test]
fn a_stale_temp_file_from_an_interrupted_import_is_replaced() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    std::fs::write(data.path().join("inventory.db.importing"), "half a copy").unwrap();
    let report = import_desktop(&desktop_db(desktop.path(), 2), data.path(), false).unwrap();
    assert_eq!(rows(&report.target), 2);
    assert!(
        leftovers(data.path()).is_empty(),
        "{:?}",
        leftovers(data.path())
    );
}

#[test]
fn messages_explain_what_to_do() {
    let exists = ImportError::TargetExists("/srv/inventory.db".into()).to_string();
    assert!(exists.contains("--replace"), "{exists}");
    let corrupt = ImportError::Corrupt(vec!["row 3 missing".into()]).to_string();
    assert!(corrupt.contains("nothing was replaced"), "{corrupt}");
}

#[test]
fn quick_successive_replaces_do_not_collide() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let source = desktop_db(desktop.path(), 1);
    import_desktop(&source, data.path(), false).unwrap();
    // Several replaces within the same second each keep their own backup.
    let kept: Vec<PathBuf> = (0..3)
        .map(|_| {
            import_desktop(&source, data.path(), true)
                .unwrap()
                .previous_kept_as
                .unwrap()
        })
        .collect();
    let unique: std::collections::HashSet<_> = kept.iter().collect();
    assert_eq!(unique.len(), 3, "{kept:?}");
    assert!(kept.iter().all(|p| p.exists()));
}

#[test]
fn only_the_newest_replaced_backups_are_kept() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let source = desktop_db(desktop.path(), 1);
    import_desktop(&source, data.path(), false).unwrap();
    for _ in 0..(KEEP_REPLACED + 3) {
        import_desktop(&source, data.path(), true).unwrap();
    }
    let backups: Vec<String> = std::fs::read_dir(data.path())
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("inventory.db.replaced-"))
        .collect();
    assert_eq!(backups.len(), KEEP_REPLACED, "{backups:?}");
}

#[test]
fn pruning_keeps_the_newest_by_time_and_counter() {
    let dir = tempfile::tempdir().unwrap();
    let names = [
        "inventory.db.replaced-20261101T100000.bak",
        "inventory.db.replaced-20261102T100000.bak",
        "inventory.db.replaced-20261102T100000-2.bak",
        "inventory.db.replaced-20261102T100000-10.bak",
        "inventory.db.replaced-20261102T100000-9.bak",
        "inventory.db",                // never touched
        "inventory-2026-11-02.db.bak", // core's daily backups are not ours to prune
    ];
    for n in names {
        std::fs::write(dir.path().join(n), "x").unwrap();
    }
    prune_replaced(dir.path(), 2).unwrap();
    let mut left: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            "inventory-2026-11-02.db.bak",
            "inventory.db",
            "inventory.db.replaced-20261102T100000-10.bak",
            "inventory.db.replaced-20261102T100000-9.bak",
        ]
    );
}

// ── --refresh: only the desktop tables are replaced ────────────────────────

/// A server database with a user, created by a plain import.
fn server_db_with_user(data: &Path, desktop: &Path, cards: usize) -> i64 {
    import_desktop(&desktop_db(desktop, cards), data, false).unwrap();
    let conn = open_writer(&inventory_db_in(data)).unwrap();
    crate::users::create(
        &conn,
        "aron",
        "Aron",
        "$hash",
        &crate::permissions::PermissionSet::all(),
        chrono::Utc::now(),
    )
    .unwrap()
}

#[test]
fn refresh_replaces_desktop_data_and_keeps_users() {
    let old_desktop = tempfile::tempdir().unwrap();
    let new_desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let user_id = server_db_with_user(data.path(), old_desktop.path(), 2);

    let report = refresh_desktop(&desktop_db(new_desktop.path(), 9), data.path()).unwrap();
    assert_eq!(report.inventory_rows, 9);
    assert_eq!(rows(&report.target), 9);
    let backup = report.previous_kept_as.expect("backup before refreshing");
    assert_eq!(rows(&backup), 2);

    let conn = Connection::open(&report.target).unwrap();
    let user = crate::users::find_by_id(&conn, user_id)
        .unwrap()
        .expect("user kept");
    assert_eq!(user.username, "aron");
    assert_eq!(
        migrations::get_meta(&conn, META_COPY_TAKEN_AT).unwrap(),
        Some(report.taken_at.clone())
    );
    let actions: Vec<String> = crate::audit::list(&conn, None, 10)
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect();
    assert_eq!(actions, ["import.refresh", "import.desktop"]);
    assert!(
        leftovers(data.path()).is_empty(),
        "{:?}",
        leftovers(data.path())
    );
}

#[test]
fn refresh_brings_indexes_and_sequences_along() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    server_db_with_user(data.path(), desktop.path(), 1);
    // A bulk lot in the fresh desktop DB: its AUTOINCREMENT counter must come along.
    let fresh = tempfile::tempdir().unwrap();
    let fresh_db = desktop_db(fresh.path(), 3);
    {
        let conn = Connection::open(&fresh_db).unwrap();
        conn.execute(
            "INSERT INTO bulk_lots (name, created_at, price) VALUES ('lot', '2026-11-01', 1.0)",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM bulk_lots", []).unwrap();
    }
    refresh_desktop(&fresh_db, data.path()).unwrap();

    let conn = Connection::open(inventory_db_in(data.path()).path()).unwrap();
    let fresh_conn = Connection::open(&fresh_db).unwrap();
    let schema = |c: &Connection| -> Vec<(String, String)> {
        let mut stmt = c
            .prepare(
                "SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'srv\\_%' ESCAPE '\\'
                 AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY type, name",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(
        schema(&conn),
        schema(&fresh_conn),
        "same tables and indexes"
    );
    let seq = |c: &Connection| -> i64 {
        c.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'bulk_lots'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(seq(&conn), seq(&fresh_conn), "ids are never reused");
    assert_eq!(
        conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
fn refresh_needs_an_existing_database() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    assert!(matches!(
        refresh_desktop(&desktop_db(desktop.path(), 1), data.path()),
        Err(ImportError::NothingToRefresh(_))
    ));
}

#[test]
fn refresh_leaves_everything_as_it_was_when_the_source_is_bad() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    server_db_with_user(data.path(), desktop.path(), 2);
    let target = data.path().join("inventory.db");
    let before = rows(&target);
    let junk = desktop.path().join("junk.db");
    std::fs::write(&junk, "definitely not a database file at all").unwrap();
    assert!(matches!(
        refresh_desktop(&junk, data.path()),
        Err(ImportError::NotAnInventoryDb(..))
    ));
    assert_eq!(rows(&target), before);
    assert!(
        leftovers(data.path()).is_empty(),
        "{:?}",
        leftovers(data.path())
    );
}

#[test]
fn refresh_refuses_a_folder_in_use() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    server_db_with_user(data.path(), desktop.path(), 1);
    let _server = DataDirLock::acquire(data.path()).unwrap();
    assert!(matches!(
        refresh_desktop(&desktop_db(desktop.path(), 1), data.path()),
        Err(ImportError::Lock(LockError::InUse(_)))
    ));
}

#[test]
fn a_plain_import_is_logged_in_the_copy() {
    let desktop = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let report = import_desktop(&desktop_db(desktop.path(), 4), data.path(), false).unwrap();
    let conn = Connection::open(&report.target).unwrap();
    let entry = &crate::audit::list(&conn, None, 1).unwrap()[0];
    assert_eq!(
        (entry.actor.as_str(), entry.action.as_str()),
        ("cli", "import.desktop")
    );
    assert_eq!(entry.details["inventory_rows"], 4);
}
