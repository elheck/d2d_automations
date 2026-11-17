use super::*;
use crate::db::Db;
use crate::password;
use crate::permissions::PermissionSet;
use crate::users;
use check_stock_core::inventory_db::InventoryDb;
use check_stock_core::io::parse_csv;

pub(crate) const HEADER: &str =
    "cardmarketId,quantity,name,set,setCode,cn,condition,language,isFoil,\
isSigned,finishType,price,comment,location,nameDE,nameES,nameFR,nameIT,rarity,listedAt";

/// A row of an export.
pub(crate) fn row(id: u32, name: &str, qty: u32, location: &str) -> String {
    format!(
        "{id},{qty},{name},Test Set,TST,{id},NM,English,,,Regular,1.00,,{location},{name} DE,,,,Common,"
    )
}

/// Bin A-0-0-1 holds 50 Keepers; A-0-0-2 three Sparks; A-0-0-3 two Embers
/// and one Keeper (the Keeper is spread over two bins).
pub(crate) fn sample_csv() -> String {
    [
        HEADER.to_string(),
        row(1, "Keeper", 50, "A-0-0-1-L1-R"),
        row(2, "Spark", 3, "A-0-0-2-L2-R"),
        row(3, "Ember", 2, "A-0-0-3-L2-R"),
        row(1, "Keeper", 1, "A-0-0-3-L3-R"),
    ]
    .join("\n")
        + "\n"
}

/// Stores `csv` as the kept import file of a new version; returns it.
pub(crate) fn keep_file(conn: &Connection, csv: &str) -> i64 {
    conn.execute(
        "INSERT INTO srv_inventory_versions (category, created_at, kind, user_name)
         VALUES ('Magic', '2026-11-15T10:00:00Z', 'import', 'Aron')",
        [],
    )
    .unwrap();
    let version = conn.last_insert_rowid();
    conn.execute(
        "INSERT OR REPLACE INTO srv_inventory_files (category, version_id, file_name, csv)
         VALUES ('Magic', ?1, 'inventory-report-Magic.csv', ?2)",
        params![version, csv.as_bytes()],
    )
    .unwrap();
    version
}

fn source(csv: &str, version: i64) -> Source {
    Source::new(
        version,
        0,
        "inventory-report-Magic.csv",
        "2026-11-15T10:00:00Z",
        parse_csv(csv.as_bytes(), 1000).unwrap(),
    )
}

fn db_with_user() -> (Db, i64, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&InventoryDb::in_dir(dir.path()), 1).unwrap();
    let conn = crate::db::open_writer(&InventoryDb::in_dir(dir.path())).unwrap();
    let hash = password::hash("a long enough password").unwrap();
    let user = users::create(
        &conn,
        "mover",
        "Mover",
        &hash,
        &PermissionSet::default(),
        Utc::now(),
    )
    .unwrap();
    (db, user, dir)
}

#[test]
fn a_consolidation_plan_empties_the_sparse_bins() {
    let plan = make_plan(
        &source(&sample_csv(), 1),
        MoveListKind::Consolidation,
        Some(20),
    );
    assert_eq!(plan.threshold, Some(20));
    assert_eq!(plan.bins_freed, ["A-0-0-2", "A-0-0-3"]);
    assert_eq!(plan.moves.len(), 3);
    assert!(plan.moves.iter().all(|m| m.planned.to_bin == "A-0-0-1"));
    // Lot and side stay.
    assert!(plan
        .moves
        .iter()
        .any(|m| m.planned.to_location == "A-0-0-1-L2-R"));
}

#[test]
fn move_ids_are_stable_and_unique() {
    let a = make_plan(&source(&sample_csv(), 1), MoveListKind::Consolidation, None);
    let b = make_plan(&source(&sample_csv(), 2), MoveListKind::Consolidation, None);
    let ids = |p: &Plan| p.moves.iter().map(|m| m.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&a), ids(&b), "same piles, same ids, in any version");
    assert_eq!(a.ids().len(), a.moves.len());
    assert!(a.moves.iter().all(|m| m.id.len() == 16));

    // The same pile listed twice still gets two ids.
    let twice = sample_csv() + &row(2, "Spark", 3, "A-0-0-2-L2-R") + "\n";
    let plan = make_plan(&source(&twice, 1), MoveListKind::Consolidation, None);
    let sparks: Vec<&str> = plan
        .moves
        .iter()
        .filter(|m| m.planned.card.name == "Spark")
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(sparks.len(), 2);
    assert_eq!(sparks[1], format!("{}-2", sparks[0]));
}

#[test]
fn a_repriced_pile_keeps_its_move_id() {
    let before = make_plan(&source(&sample_csv(), 1), MoveListKind::Consolidation, None);
    let repriced = sample_csv().replace(
        &row(2, "Spark", 3, "A-0-0-2-L2-R"),
        "2,3,Spark,Test Set,TST,2,NM,English,,,Regular,2.75,new comment,A-0-0-2-L2-R,,,,,Common,",
    );
    let after = make_plan(&source(&repriced, 2), MoveListKind::Consolidation, None);
    let spark = |p: &Plan| {
        p.moves
            .iter()
            .find(|m| m.planned.card.name == "Spark")
            .map(|m| (m.id.clone(), m.planned.card.price.clone()))
            .unwrap()
    };
    assert_eq!(spark(&before).0, spark(&after).0);
    assert_eq!(spark(&after).1, "2.75", "the plan has the current values");
}

#[test]
fn a_defrag_plan_gathers_the_spread_variant() {
    let plan = make_plan(&source(&sample_csv(), 1), MoveListKind::Defrag, Some(5));
    assert_eq!(plan.threshold, None, "no threshold for a defrag");
    assert_eq!(plan.fragmented_variants, 1);
    assert_eq!(plan.moves.len(), 1);
    assert_eq!(plan.moves[0].planned.from_location, "A-0-0-3-L3-R");
    assert_eq!(plan.moves[0].planned.to_location, "A-0-0-1-L3-R");
}

#[tokio::test]
async fn ticks_carry_over_to_a_new_list_where_the_move_stays() {
    let (db, user, _dir) = db_with_user();
    let first = make_plan(
        &source(&sample_csv(), 1),
        MoveListKind::Consolidation,
        Some(20),
    );
    assert_eq!(first.moves.len(), 3);
    let spark = |p: &Plan| {
        p.moves
            .iter()
            .find(|m| m.planned.card.name == "Spark")
            .unwrap()
            .id
            .clone()
    };
    // The next import: the Embers grew to 30, so their bin is no longer emptied.
    let fuller = sample_csv().replace(
        &row(3, "Ember", 2, "A-0-0-3-L2-R"),
        &row(3, "Ember", 30, "A-0-0-3-L2-R"),
    );
    let second = make_plan(&source(&fuller, 2), MoveListKind::Consolidation, Some(20));
    assert_eq!(second.moves.len(), 1);
    assert_eq!(spark(&first), spark(&second));
    let expected = spark(&second);

    let all: Vec<String> = first.moves.iter().map(|m| m.id.clone()).collect();
    let (ticked_first, carried, dropped, ticked, stored) = db
        .write(move |conn| {
            save_list(conn, user, &first, Utc::now())?;
            let ticked_first = set_ticks(conn, user, &all, true)?;
            let (carried, dropped) = save_list(conn, user, &second, Utc::now())?;
            Ok((
                ticked_first,
                carried,
                dropped,
                ticks(conn, user)?,
                load_list(conn, user)?,
            ))
        })
        .await
        .unwrap();
    assert_eq!(ticked_first, 3);
    assert_eq!((carried, dropped), (1, 2));
    assert_eq!(ticked, HashSet::from([expected]));
    assert_eq!(stored.unwrap().version, 2);
}

#[tokio::test]
async fn discarding_a_list_removes_its_ticks() {
    let (db, user, _dir) = db_with_user();
    let plan = make_plan(&source(&sample_csv(), 1), MoveListKind::Defrag, None);
    let left = db
        .write(move |conn| {
            save_list(conn, user, &plan, Utc::now())?;
            set_ticks(conn, user, &[plan.moves[0].id.clone()], true)?;
            assert!(discard_list(conn, user)?);
            assert!(!discard_list(conn, user)?);
            conn.query_row("SELECT COUNT(*) FROM srv_move_list_ticks", [], |r| {
                r.get::<_, i64>(0)
            })
        })
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn plans_are_cached_per_file_and_discards() {
    let service = BinsService::default();
    let first = Arc::new(source(&sample_csv(), 1));
    let plan = service
        .plan(first.clone(), MoveListKind::Consolidation, None)
        .await
        .unwrap();
    let same = service
        .plan(
            first.clone(),
            MoveListKind::Consolidation,
            Some(DEFAULT_MERGE_THRESHOLD),
        )
        .await
        .unwrap();
    assert!(
        Arc::ptr_eq(&plan, &same),
        "default threshold is the same plan"
    );
    // The same file after a discard: another plan.
    let discarded = Arc::new(Source::new(
        1,
        5,
        "inventory-report-Magic.csv",
        "2026-11-15T10:00:00Z",
        first.cards.clone(),
    ));
    let after = service
        .plan(discarded, MoveListKind::Consolidation, None)
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(&plan, &after));
    assert_eq!(after.discard_seq, 5);
    for threshold in 1..=MAX_CACHED_PLANS as i64 {
        service
            .plan(
                first.clone(),
                MoveListKind::Consolidation,
                Some(threshold + 30),
            )
            .await
            .unwrap();
    }
    let recomputed = service
        .plan(first.clone(), MoveListKind::Consolidation, None)
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(&plan, &recomputed), "evicted after 8 others");
}
