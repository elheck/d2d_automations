use super::*;
use crate::db::{open_writer, Db};
use crate::password;
use crate::permissions::PermissionSet;
use crate::users;
use check_stock_core::inventory_db::InventoryDb;
use chrono::TimeZone;

const HEADER: &str =
    "cardmarketId,quantity,name,set,setCode,cn,condition,language,isFoil,isSigned,\
finishType,price,comment,location,nameDE,nameES,nameFR,nameIT,rarity,listedAt";
const MAGIC: &str = "inventory-report-Magic.csv";

/// An export with cards `1..=n`, one copy each (location `A-0-0-<id>-L1-R`).
fn export(n: usize) -> String {
    export_with(n, "1")
}

fn export_with(n: usize, quantity: &str) -> String {
    let mut csv = format!("{HEADER}\n");
    for i in 1..=n {
        csv.push_str(&format!(
            "{i},{quantity},Card {i},Test Set,TST,{i},NM,English,,,Regular,1.00,,A-0-0-{i}-L1-R,,,,,Common,\n"
        ));
    }
    csv
}

fn file(n: usize) -> ImportFile {
    read_file(MAGIC, export(n)).unwrap()
}

/// Fields drop in order: the connection closes before the folder is
/// removed (Windows cannot delete open files).
struct Fixture {
    conn: Connection,
    db_path: PathBuf,
    anna: i64,
    ben: i64,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let inventory = InventoryDb::in_dir(dir.path());
    drop(Db::open(&inventory, 1).unwrap());
    let conn = open_writer(&inventory).unwrap();
    let hash = password::hash("a long enough password").unwrap();
    let add = |name: &str| {
        users::create(
            &conn,
            name,
            name,
            &hash,
            &PermissionSet::default(),
            Utc::now(),
        )
        .unwrap()
    };
    let (anna, ben) = (add("anna"), add("ben"));
    Fixture {
        db_path: inventory.path().to_path_buf(),
        _dir: dir,
        conn,
        anna,
        ben,
    }
}

fn importer(id: i64, name: &str) -> Importer<'_> {
    Importer {
        id,
        username: name,
        display_name: name,
    }
}

fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 1, 10, minute, 0).unwrap()
}

impl Fixture {
    fn preview(&self, user: i64, file: &ImportFile, today: &str, now: DateTime<Utc>) -> Checked {
        let checked = check(&self.conn, file, today).unwrap();
        save_pending(&self.conn, user, file, &checked, now).unwrap();
        checked
    }

    fn confirm(
        &mut self,
        user: i64,
        file: &ImportFile,
        typed: Option<&str>,
        today: &str,
        now: DateTime<Utc>,
    ) -> Result<Confirmed, ConfirmError> {
        let name = if user == self.anna { "anna" } else { "ben" };
        let db_path = self.db_path.clone();
        confirm(
            &mut self.conn,
            &db_path,
            importer(user, name),
            file,
            Typed {
                copies_sold: typed,
                returning: None,
            },
            now,
            today,
        )
        .unwrap()
    }

    /// Preview and confirm in one go.
    fn import(&mut self, user: i64, file: &ImportFile, today: &str) -> Confirmed {
        self.preview(user, file, today, at(0));
        self.confirm(user, file, None, today, at(1)).unwrap()
    }

    fn copies(&self) -> i64 {
        self.conn
            .query_row(
                "SELECT COALESCE(SUM(quantity), 0) FROM inventory_cards",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn count(&self, table: &str) -> i64 {
        self.conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn audit(&self) -> Vec<(String, String)> {
        crate::audit::list(&self.conn, None, 100)
            .unwrap()
            .into_iter()
            .map(|e| (e.action, e.target.unwrap_or_default()))
            .collect()
    }

    fn backups(&self) -> Vec<String> {
        let dir = self.db_path.parent().unwrap();
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.starts_with("inventory.db.import-"))
            .collect();
        names.sort();
        names
    }
}

// ── Reading the file ───────────────────────────────────────────────────────

#[test]
fn reads_an_export_with_its_category_and_fingerprint() {
    let f = file(3);
    assert_eq!(f.category, "Magic");
    assert_eq!(f.cards.len(), 3);
    assert_eq!(f.copies(), 3);
    assert_eq!(f.sha256.len(), 64);
    assert_eq!(f.sha256, file(3).sha256);
    assert_ne!(f.sha256, file(4).sha256);
    let generic = read_file("inventory-report-Generic.csv", export(1)).unwrap();
    assert_eq!(generic.category, "Generic");
}

#[test]
fn keeps_only_the_file_name_without_folders() {
    let f = read_file(
        r"C:\Users\x\Downloads\inventory-report-Magic.csv",
        export(1),
    )
    .unwrap();
    assert_eq!(f.file_name, MAGIC);
    let f = read_file("/home/x/inventory-report-Magic.csv", export(1)).unwrap();
    assert_eq!(f.file_name, MAGIC);
    for bad in ["", "  ", "folder/", "a\u{0}b.csv", &"x".repeat(201)] {
        assert_eq!(
            read_file(bad, export(1)).unwrap_err(),
            FileError::BadFileName,
            "{bad:?}"
        );
    }
}

#[test]
fn refuses_unknown_categories_in_the_file_name() {
    for name in [
        "inventory-report-<b>x.csv",
        "inventory-report-Magic Cards!.csv",
        &format!("inventory-report-{}.csv", "A".repeat(41)),
    ] {
        assert_eq!(
            read_file(name, export(1)).unwrap_err(),
            FileError::BadCategory,
            "{name}"
        );
    }
}

#[test]
fn refuses_files_that_are_not_exports_or_too_large() {
    let err = read_file(MAGIC, "name,price\nBolt,1\n".to_string()).unwrap_err();
    assert!(matches!(err, FileError::NotAnExport { .. }), "{err:?}");
    assert!(!err.to_string().contains("Bolt"), "no file content: {err}");
    let huge = "x".repeat(MAX_IMPORT_BYTES + 1);
    assert_eq!(read_file(MAGIC, huge).unwrap_err(), FileError::TooLarge);
}

#[test]
fn refuses_quantities_and_prices_out_of_range() {
    for (quantity, price) in [
        ("-3", "1.00"),
        ("1.5", "1.00"),
        ("abc", "1.00"),
        ("1000001", "1.00"),
        ("99999999999999999999", "1.00"),
        ("1", "-1"),
        ("1", "1e400"),
        ("1", "NaN"),
        ("1", "1000000.01"),
        ("1", "cheap"),
    ] {
        let csv = format!(
            "{HEADER}\n1,{quantity},Bolt,Test Set,TST,1,NM,English,,,Regular,{price},,,,,,,Common,\n"
        );
        assert_eq!(
            read_file(MAGIC, csv).unwrap_err(),
            FileError::BadValue {
                card: "Bolt".to_string()
            },
            "{quantity} / {price}"
        );
    }
    // Comma decimals are fine (the desktop reads them the same way).
    let csv =
        format!("{HEADER}\n1,2,Bolt,Test Set,TST,1,NM,English,,,Regular,\"1,50\",,,,,,,Common,\n");
    assert!(read_file(MAGIC, csv).is_ok());
}

#[test]
fn typed_counts_allow_separators_only() {
    assert_eq!(parse_typed_count("1234"), Some(1234));
    assert_eq!(parse_typed_count(" 1,234 "), Some(1234));
    assert_eq!(parse_typed_count("1.234"), Some(1234));
    assert_eq!(parse_typed_count("1 234"), Some(1234));
    for bad in ["", " ", "12a", "-5", "1e3", "١٢"] {
        assert_eq!(parse_typed_count(bad), None, "{bad:?}");
    }
}

// ── Preview and confirmation ───────────────────────────────────────────────

#[test]
fn a_confirmed_import_writes_data_version_file_and_audit_together() {
    let mut fx = fixture();
    let anna = fx.anna;
    let first = fx.import(anna, &file(3), "2026-03-01");
    assert_eq!(first.version, 1);
    assert!(!first.forced);
    assert_eq!(first.numbers.new_variants, 3);
    assert_eq!(fx.copies(), 3);

    let second = file(2);
    let checked = fx.preview(anna, &second, "2026-03-02", at(5));
    assert_eq!(checked.numbers.copies_sold, 1);
    assert_eq!(checked.base_version, 1);
    assert_eq!(fx.count("srv_pending_imports"), 1);
    let done = fx
        .confirm(anna, &second, None, "2026-03-02", at(6))
        .unwrap();
    assert_eq!(done.version, 2);
    assert_eq!(done.numbers.copies_sold, 1);
    assert_eq!(done.zeroed, 1);
    assert_eq!(fx.copies(), 2);
    assert_eq!(fx.count("srv_pending_imports"), 0);

    let versions = recent_versions(&fx.conn, 10).unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].kind, "import");
    assert_eq!(versions[0].user_name, "anna");
    assert_eq!(versions[0].copies_sold, 1);
    assert_eq!(latest_version(&fx.conn).unwrap().0, 2);

    let (name, csv, version): (String, Vec<u8>, i64) = fx
        .conn
        .query_row(
            "SELECT file_name, csv, version_id FROM srv_inventory_files WHERE category = 'Magic'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((name.as_str(), version), (MAGIC, 2));
    assert_eq!(csv, export(2).into_bytes(), "the file of the last import");
    assert_eq!(fx.count("srv_inventory_files"), 1, "replaced, not added");

    assert_eq!(
        fx.audit(),
        [
            ("inventory.imported".to_string(), "Magic".to_string()),
            ("inventory.imported".to_string(), "Magic".to_string()),
        ]
    );
    assert_eq!(fx.backups().len(), 2, "a backup before each import");
}

#[test]
fn the_backup_holds_the_data_from_before_the_import() {
    let mut fx = fixture();
    let anna = fx.anna;
    fx.import(anna, &file(3), "2026-03-01");
    let done = fx.import(anna, &file(1), "2026-03-02");
    let backup = fx.db_path.with_file_name(&done.backup);
    let copy = Connection::open(&backup).unwrap();
    let copies: i64 = copy
        .query_row("SELECT SUM(quantity) FROM inventory_cards", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(copies, 3);
    assert_eq!(fx.copies(), 1);
}

#[test]
fn only_the_newest_ten_import_backups_are_kept() {
    let fx = fixture();
    let dir = fx.db_path.parent().unwrap();
    for day in 1..=12 {
        std::fs::write(
            dir.join(format!("inventory.db.import-202601{day:02}T100000.bak")),
            "old",
        )
        .unwrap();
    }
    std::fs::write(dir.join("inventory.db.replaced-20260101T100000.bak"), "x").unwrap();
    let made = backup_before_import(&fx.conn, &fx.db_path).unwrap();
    let left = fx.backups();
    assert_eq!(left.len(), KEEP_IMPORT_BACKUPS);
    assert!(left.contains(&made.file_name().unwrap().to_string_lossy().to_string()));
    assert!(!left.contains(&"inventory.db.import-20260103T100000.bak".to_string()));
    assert!(dir
        .join("inventory.db.replaced-20260101T100000.bak")
        .exists());
}

#[test]
fn without_a_backup_nothing_is_imported() {
    let mut fx = fixture();
    let anna = fx.anna;
    let f = file(3);
    fx.preview(anna, &f, "2026-03-01", at(0));
    fx.db_path = fx.db_path.with_file_name("missing-folder/inventory.db");
    assert_eq!(
        fx.confirm(anna, &f, None, "2026-03-01", at(1)),
        Err(ConfirmError::BackupFailed)
    );
    assert_eq!(fx.copies(), 0);
    assert_eq!(fx.count("srv_inventory_versions"), 0);
    assert_eq!(fx.count("srv_pending_imports"), 1, "can be retried");
    assert!(fx.audit().is_empty());
}

#[test]
fn confirming_needs_a_fresh_preview_of_the_same_file() {
    let mut fx = fixture();
    let anna = fx.anna;
    let f = file(3);
    assert_eq!(
        fx.confirm(anna, &f, None, "2026-03-01", at(0)),
        Err(ConfirmError::NoPreview)
    );
    fx.preview(anna, &f, "2026-03-01", at(0));
    assert_eq!(
        fx.confirm(anna, &file(4), None, "2026-03-01", at(1)),
        Err(ConfirmError::OtherFile)
    );
    let generic = read_file("inventory-report-Generic.csv", export(3)).unwrap();
    assert_eq!(
        fx.confirm(anna, &generic, None, "2026-03-01", at(1)),
        Err(ConfirmError::OtherFile),
        "same rows, other category"
    );
    // 30 minutes later the preview is gone.
    assert_eq!(
        fx.confirm(anna, &f, None, "2026-03-01", at(30)),
        Err(ConfirmError::Expired)
    );
    assert_eq!(fx.count("srv_pending_imports"), 0);
    assert_eq!(fx.copies(), 0);
}

#[test]
fn previews_are_per_user_and_expired_ones_are_cleaned_up() {
    let fx = fixture();
    fx.preview(fx.anna, &file(3), "2026-03-01", at(0));
    fx.preview(fx.anna, &file(2), "2026-03-01", at(1));
    assert_eq!(fx.count("srv_pending_imports"), 1, "replaced");
    fx.preview(fx.ben, &file(2), "2026-03-01", at(40));
    assert_eq!(
        fx.count("srv_pending_imports"),
        1,
        "anna's expired one removed"
    );
    assert!(discard_pending(&fx.conn, fx.ben).unwrap());
    assert!(!discard_pending(&fx.conn, fx.ben).unwrap());
}

#[test]
fn another_import_in_between_makes_a_preview_outdated() {
    let mut fx = fixture();
    let (anna, ben) = (fx.anna, fx.ben);
    fx.import(anna, &file(5), "2026-03-01");
    let annas = file(4);
    fx.preview(anna, &annas, "2026-03-02", at(0));
    fx.import(ben, &file(3), "2026-03-02");
    assert_eq!(
        fx.confirm(anna, &annas, None, "2026-03-02", at(2)),
        Err(ConfirmError::Outdated)
    );
    assert_eq!(fx.copies(), 3);
}

#[test]
fn a_preview_is_refused_when_the_numbers_changed() {
    let mut fx = fixture();
    let anna = fx.anna;
    fx.import(anna, &file(5), "2026-03-01");
    let f = file(4);
    fx.preview(anna, &f, "2026-03-02", at(0));
    // Something else changed the stock (e.g. a discard, later in Phase 4).
    fx.conn
        .execute(
            "UPDATE inventory_cards SET quantity = 3 WHERE cardmarket_id = '5'",
            [],
        )
        .unwrap();
    assert_eq!(
        fx.confirm(anna, &f, None, "2026-03-02", at(1)),
        Err(ConfirmError::Changed)
    );
}

#[test]
fn a_blocked_import_needs_the_typed_number_and_is_logged_as_forced() {
    let mut fx = fixture();
    let anna = fx.anna;
    fx.import(anna, &file(200), "2026-03-01");
    let cut_off = file(50);
    let checked = fx.preview(anna, &cut_off, "2026-03-02", at(0));
    assert!(checked.blocked);
    assert_eq!(checked.numbers.copies_sold, 150);
    assert_eq!(
        fx.confirm(anna, &cut_off, None, "2026-03-02", at(1)),
        Err(ConfirmError::NeedsConfirmation { copies_sold: 150 })
    );
    assert_eq!(
        fx.confirm(anna, &cut_off, Some("149"), "2026-03-02", at(1)),
        Err(ConfirmError::WrongConfirmation { copies_sold: 150 })
    );
    assert_eq!(
        fx.confirm(anna, &cut_off, Some("yes"), "2026-03-02", at(1)),
        Err(ConfirmError::WrongConfirmation { copies_sold: 150 })
    );
    assert_eq!(fx.copies(), 200);
    let done = fx
        .confirm(anna, &cut_off, Some("150"), "2026-03-02", at(1))
        .unwrap();
    assert!(done.forced);
    assert_eq!(fx.copies(), 50);
    assert_eq!(
        recent_versions(&fx.conn, 1).unwrap()[0].kind,
        "forced_import"
    );
    assert_eq!(
        fx.audit()[0],
        ("inventory.import_forced".to_string(), "Magic".to_string())
    );
}

#[test]
fn a_typed_number_is_ignored_when_nothing_is_blocked() {
    let mut fx = fixture();
    let anna = fx.anna;
    let f = file(3);
    fx.preview(anna, &f, "2026-03-01", at(0));
    let done = fx
        .confirm(anna, &f, Some("whatever"), "2026-03-01", at(1))
        .unwrap();
    assert!(!done.forced);
}

#[test]
fn unusually_many_sales_are_a_warning_not_a_block() {
    let mut fx = fixture();
    let anna = fx.anna;
    fx.import(anna, &file(200), "2026-03-01");
    fx.import(anna, &file(190), "2026-03-10");
    // 10 copies in 9 days; one day later 40 are gone.
    let checked = fx.preview(anna, &file(150), "2026-03-11", at(0));
    assert!(!checked.blocked);
    let unusual = checked.unusual.expect("warning");
    assert_eq!(unusual.days_since_last_import, 1);
    assert_eq!(checked.pace.unwrap().days, 9);
    assert!(fx
        .confirm(anna, &file(150), None, "2026-03-11", at(1))
        .is_ok());
}

#[test]
fn a_new_category_is_flagged() {
    let mut fx = fixture();
    let anna = fx.anna;
    fx.import(anna, &file(3), "2026-03-01");
    let generic = read_file("inventory-report-Generic.csv", export(2)).unwrap();
    let checked = check(&fx.conn, &generic, "2026-03-01").unwrap();
    assert!(checked.new_category);
    assert_eq!(checked.base_version, 0);
    assert!(
        !check(&fx.conn, &file(3), "2026-03-01")
            .unwrap()
            .new_category
    );
}

#[test]
fn replaced_data_gets_a_version_per_category_and_drops_previews() {
    let mut fx = fixture();
    let anna = fx.anna;
    fx.import(anna, &file(3), "2026-03-01");
    fx.preview(anna, &file(2), "2026-03-02", at(0));
    let categories = record_replaced_data(&fx.conn, "restore", at(5), true).unwrap();
    assert_eq!(categories, ["Magic"]);
    assert_eq!(fx.count("srv_pending_imports"), 0);
    assert_eq!(fx.count("srv_inventory_files"), 1, "kept for a restore");
    let newest = &recent_versions(&fx.conn, 1).unwrap()[0];
    assert_eq!(
        (newest.kind.as_str(), newest.user_name.as_str()),
        ("restore", "command line")
    );
    record_replaced_data(&fx.conn, "desktop_copy", at(6), false).unwrap();
    assert_eq!(fx.count("srv_inventory_files"), 0);
}

#[test]
fn a_deleted_users_imports_keep_their_name() {
    let mut fx = fixture();
    let anna = fx.anna;
    fx.import(anna, &file(1), "2026-03-01");
    fx.conn
        .execute("DELETE FROM srv_users WHERE id = ?1", [anna])
        .unwrap();
    assert_eq!(recent_versions(&fx.conn, 1).unwrap()[0].user_name, "anna");
}
