use super::*;
use crate::bins::tests::{keep_file, row, sample_csv, HEADER};
use crate::db::{open_writer, Db};
use crate::inventory_import::{self, check, read_file, save_pending, Typed};
use crate::password;
use crate::permissions::PermissionSet;
use crate::users;
use check_stock_core::category::DEFAULT_CATEGORY;
use check_stock_core::inventory_db::{sync_inventory_on_conn, InventoryDb};
use check_stock_core::io::parse_csv;

/// The database knows the sample stock (synced on 2026-11-14), and the same
/// file is kept as the last import.
struct Fx {
    conn: Connection,
    db_path: PathBuf,
    user: i64,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let inventory = InventoryDb::in_dir(dir.path());
    drop(Db::open(&inventory, 1).unwrap());
    let mut conn = open_writer(&inventory).unwrap();
    let hash = password::hash("a long enough password").unwrap();
    let user = users::create(
        &conn,
        "anna",
        "Anna",
        &hash,
        &PermissionSet::default(),
        Utc::now(),
    )
    .unwrap();
    let cards = parse_csv(sample_csv().as_bytes(), 100).unwrap();
    sync_inventory_on_conn(&mut conn, &cards, DEFAULT_CATEGORY, "2026-11-14").unwrap();
    keep_file(&conn, &sample_csv());
    Fx {
        conn,
        db_path: inventory.path().to_path_buf(),
        user,
        _dir: dir,
    }
}

impl Fx {
    fn source(&self) -> Source {
        let parsed = {
            let cards = parse_csv(sample_csv().as_bytes(), 100).unwrap();
            let ids = crate::stock::pile_ids(&cards);
            crate::stock::Parsed {
                version: 1,
                file_name: "inventory-report-Magic.csv".into(),
                imported_at: "2026-11-15T10:00:00Z".into(),
                cards,
                ids,
            }
        };
        crate::stock::apply_discards(&parsed, &pending(&self.conn, "Magic").unwrap())
    }

    fn discard(
        &mut self,
        source: &Source,
        items: &[(&str, i64)],
    ) -> Result<Discarded, DiscardError> {
        let items: Vec<(String, i64)> = items.iter().map(|(p, q)| (p.to_string(), *q)).collect();
        let db_path = self.db_path.clone();
        let user = self.user;
        discard(
            &mut self.conn,
            &db_path,
            Importer {
                id: user,
                username: "anna",
                display_name: "Anna",
            },
            source,
            &items,
            Utc::now(),
        )
        .unwrap()
    }

    fn stock(&self, id: &str) -> i64 {
        self.conn
            .query_row(
                "SELECT SUM(quantity) FROM inventory_cards WHERE cardmarket_id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn sold(&self) -> i64 {
        self.conn
            .query_row("SELECT SUM(sold_quantity) FROM inventory_cards", [], |r| {
                r.get(0)
            })
            .unwrap()
    }

    fn backups(&self) -> usize {
        std::fs::read_dir(self.db_path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.starts_with("inventory.db.discard-"))
            .count()
    }
}

#[test]
fn a_discard_lowers_the_stock_without_a_sale_and_records_everything() {
    let mut fx = fixture();
    let source = fx.source();
    let spark = source.ids[1].clone();
    let done = fx.discard(&source, &[(&spark, 2)]).unwrap();
    assert_eq!((done.copies, done.variants), (2, 1));
    assert!((done.value - 2.0).abs() < 1e-9);
    assert_eq!(fx.stock("2"), 1);
    assert_eq!(fx.sold(), 0, "not counted as sold");
    assert_eq!(fx.backups(), 1);

    let csv = String::from_utf8(csv_of(&fx.conn, done.id).unwrap().unwrap()).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(
        lines[1].starts_with("2,-2,Spark,"),
        "negative quantity: {}",
        lines[1]
    );
    assert!(lines[1].contains("A-0-0-2-L2-R"));

    let history = recent(&fx.conn, 5).unwrap();
    assert_eq!(history[0].copies, 2);
    assert!(!history[0].settled && history[0].csv_available);
    let entry = &crate::audit::list(&fx.conn, None, 1).unwrap()[0];
    assert_eq!(entry.action, "inventory.discarded");
    assert_eq!(entry.details["piles"][0]["location"], "A-0-0-2-L2-R");

    // The stock seen by the screens has the pile reduced.
    assert_eq!(fx.source().pile(&spark).unwrap().quantity, "1");
}

#[test]
fn more_than_is_left_is_refused_not_capped() {
    let mut fx = fixture();
    let source = fx.source();
    let spark = source.ids[1].clone();
    assert_eq!(
        fx.discard(&source, &[(&spark, 4)]),
        Err(DiscardError::NotEnough {
            name: "Spark".into(),
            available: 3
        })
    );
    // Two discards from the same (old) search: the second sees the first.
    fx.discard(&source, &[(&spark, 2)]).unwrap();
    assert_eq!(
        fx.discard(&source, &[(&spark, 2)]),
        Err(DiscardError::NotEnough {
            name: "Spark".into(),
            available: 1
        })
    );
    // The same pile twice in one request counts twice.
    let fresh = fx.source();
    assert!(matches!(
        fx.discard(&fresh, &[(&spark, 1), (&spark, 1)]),
        Err(DiscardError::NotEnough { .. })
    ));
    assert_eq!(fx.stock("2"), 1);
}

#[test]
fn the_pile_must_have_the_copies_even_when_the_card_has_more_elsewhere() {
    let mut fx = fixture();
    let source = fx.source();
    // The Keeper: 50 in A-0-0-1, 1 in A-0-0-3 (51 in the database).
    let small_pile = source.ids[3].clone();
    assert_eq!(source.pile(&small_pile).unwrap().quantity, "1");
    assert_eq!(
        fx.discard(&source, &[(&small_pile, 2)]),
        Err(DiscardError::NotEnough {
            name: "Keeper".into(),
            available: 1
        })
    );
    assert_eq!(fx.stock("1"), 51);
}

#[test]
fn a_second_discard_from_an_old_search_sees_the_first() {
    let mut fx = fixture();
    let old_search = fx.source();
    let small_pile = old_search.ids[3].clone();
    fx.discard(&old_search, &[(&small_pile, 1)]).unwrap();
    // The database still has 50 Keepers, but that pile is empty now.
    assert_eq!(
        fx.discard(&old_search, &[(&small_pile, 1)]),
        Err(DiscardError::NotEnough {
            name: "Keeper".into(),
            available: 0
        })
    );
    assert_eq!(fx.stock("1"), 50);
}

#[test]
fn the_database_must_have_the_copies_too() {
    let mut fx = fixture();
    let source = fx.source();
    fx.conn
        .execute(
            "UPDATE inventory_cards SET quantity = 1 WHERE cardmarket_id = '2'",
            [],
        )
        .unwrap();
    assert_eq!(
        fx.discard(&source, &[(&source.ids[1], 2)]),
        Err(DiscardError::NotEnough {
            name: "Spark".into(),
            available: 1
        })
    );
}

#[test]
fn bad_requests_change_nothing() {
    let mut fx = fixture();
    let source = fx.source();
    assert_eq!(fx.discard(&source, &[]), Err(DiscardError::Empty));
    assert_eq!(
        fx.discard(&source, &[("0123456789abcdef", 1)]),
        Err(DiscardError::UnknownPile)
    );
    assert_eq!(
        fx.discard(&source, &[(&source.ids[1], 0)]),
        Err(DiscardError::BadQuantity)
    );
    // A new import since the search.
    keep_file(&fx.conn, &sample_csv());
    assert_eq!(
        fx.discard(&source, &[(&source.ids[1], 1)]),
        Err(DiscardError::Changed)
    );
    assert_eq!(fx.stock("2"), 3);
    assert_eq!(fx.backups(), 0);
    assert!(recent(&fx.conn, 5).unwrap().is_empty());
}

#[test]
fn without_a_backup_nothing_is_discarded() {
    let mut fx = fixture();
    let source = fx.source();
    fx.db_path = fx.db_path.with_file_name("missing/inventory.db");
    assert_eq!(
        fx.discard(&source, &[(&source.ids[1], 1)]),
        Err(DiscardError::BackupFailed)
    );
    assert_eq!(fx.stock("2"), 3);
    assert!(recent(&fx.conn, 5).unwrap().is_empty());
}

#[test]
fn only_the_newest_20_csvs_are_kept() {
    let mut fx = fixture();
    for _ in 0..21 {
        let source = fx.source();
        fx.discard(&source, &[(&source.ids[0], 1)]).unwrap();
    }
    let history = recent(&fx.conn, 30).unwrap();
    assert_eq!(history.len(), 21);
    assert!(history[..20].iter().all(|d| d.csv_available));
    assert!(!history[20].csv_available);
    assert_eq!(csv_of(&fx.conn, history[20].id).unwrap(), None);
    assert_eq!(fx.backups(), KEEP_DISCARD_BACKUPS);
}

#[test]
fn an_old_export_that_brings_discards_back_needs_the_typed_number() {
    let mut fx = fixture();
    let source = fx.source();
    fx.discard(&source, &[(&source.ids[1], 2)]).unwrap();
    // The export from before the discard CSV went into Cardmarket.
    let old = read_file("inventory-report-Magic.csv", sample_csv()).unwrap();
    assert_eq!(returning_copies(&fx.conn, "Magic", &old.cards).unwrap(), 2);
    // A fresh export (Sparks gone) brings nothing back.
    let fresh_csv = sample_csv().replace(
        &row(2, "Spark", 3, "A-0-0-2-L2-R"),
        &row(2, "Spark", 1, "A-0-0-2-L2-R"),
    );
    let fresh = read_file("inventory-report-Magic.csv", fresh_csv).unwrap();
    assert_eq!(
        returning_copies(&fx.conn, "Magic", &fresh.cards).unwrap(),
        0
    );
    // Other categories are not affected.
    assert_eq!(
        returning_copies(&fx.conn, "Generic", &old.cards).unwrap(),
        0
    );

    let checked = check(&fx.conn, &old, "2026-11-15").unwrap();
    assert_eq!(checked.returning_discards, 2);
    save_pending(&fx.conn, fx.user, &old, &checked, Utc::now()).unwrap();
    let db_path = fx.db_path.clone();
    let user = fx.user;
    let mut confirm = |typed: Typed<'_>| {
        inventory_import::confirm(
            &mut fx.conn,
            &db_path,
            Importer {
                id: user,
                username: "anna",
                display_name: "Anna",
            },
            &old,
            typed,
            Utc::now(),
            "2026-11-15",
        )
        .unwrap()
    };
    assert_eq!(
        confirm(Typed::default()).unwrap_err(),
        inventory_import::ConfirmError::NeedsReturningConfirmation { copies: 2 }
    );
    assert_eq!(
        confirm(Typed {
            returning: Some("3"),
            ..Typed::default()
        })
        .unwrap_err(),
        inventory_import::ConfirmError::WrongReturningConfirmation { copies: 2 }
    );
    confirm(Typed {
        returning: Some("2"),
        ..Typed::default()
    })
    .unwrap();
    // The import settled the discard: nothing pending any more.
    assert!(pending(&fx.conn, "Magic").unwrap().is_empty());
    assert!(recent(&fx.conn, 1).unwrap()[0].settled);
    assert_eq!(fx.stock("2"), 3, "the copies are back, as confirmed");
}

#[test]
fn a_restore_settles_pending_discards() {
    let mut fx = fixture();
    let source = fx.source();
    fx.discard(&source, &[(&source.ids[1], 1)]).unwrap();
    inventory_import::record_replaced_data(&fx.conn, "restore", Utc::now(), true).unwrap();
    assert!(pending(&fx.conn, "Magic").unwrap().is_empty());
}

#[test]
fn the_data_version_grows_with_discards() {
    let mut fx = fixture();
    let before = inventory_import::latest_version(&fx.conn).unwrap().0;
    let source = fx.source();
    fx.discard(&source, &[(&source.ids[1], 1)]).unwrap();
    let after = inventory_import::latest_version(&fx.conn).unwrap();
    assert_eq!(after.0, before + 1);
    assert!(after.1.is_some());
}

#[test]
fn header_row_is_the_inventory_schema() {
    // The discard CSV is imported by TCG Powertools: same header as the
    // stock-update CSV of the desktop app.
    let mut fx = fixture();
    let source = fx.source();
    let done = fx.discard(&source, &[(&source.ids[0], 1)]).unwrap();
    let csv = String::from_utf8(csv_of(&fx.conn, done.id).unwrap().unwrap()).unwrap();
    assert!(csv.starts_with("cardmarketId,quantity,name,set,setCode,cn,condition,language,"));
    let _ = HEADER;
}
