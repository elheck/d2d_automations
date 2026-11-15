use super::*;
use crate::bins::tests::{keep_file, row, sample_csv};
use check_stock_core::inventory_db::InventoryDb;

fn cards(csv: &str) -> Vec<Card> {
    parse_csv(csv.as_bytes(), 1000).unwrap()
}

fn parsed(csv: &str) -> Parsed {
    let cards = cards(csv);
    let ids = pile_ids(&cards);
    Parsed {
        version: 1,
        file_name: "f.csv".into(),
        imported_at: "2026-11-15T10:00:00Z".into(),
        cards,
        ids,
    }
}

fn item(pile: &str, quantity: i64) -> DiscardedPile {
    DiscardedPile {
        pile: pile.into(),
        quantity,
        cardmarket_id: String::new(),
        name: String::new(),
        set_code: String::new(),
        condition: String::new(),
        language: String::new(),
        is_foil: String::new(),
        is_signed: String::new(),
        location: String::new(),
        price: String::new(),
    }
}

#[test]
fn pile_ids_are_stable_and_unique() {
    let a = pile_ids(&cards(&sample_csv()));
    assert_eq!(a, pile_ids(&cards(&sample_csv())));
    assert_eq!(a.iter().collect::<HashSet<_>>().len(), a.len());
    assert!(a.iter().all(|id| id.len() == 16));
    // The Keeper in two bins: two piles, two ids.
    assert_ne!(a[0], a[3]);
    // An identical row gets a counter.
    let twice = sample_csv() + &row(2, "Spark", 3, "A-0-0-2-L2-R") + "\n";
    let b = pile_ids(&cards(&twice));
    assert_eq!(b[4], format!("{}-2", b[1]));
}

#[test]
fn discards_are_taken_off_their_piles() {
    let p = parsed(&sample_csv());
    let spark = p.ids[1].clone();
    let ember = p.ids[2].clone();
    let source = apply_discards(
        &p,
        &[(3, vec![item(&spark, 1)]), (7, vec![item(&ember, 2)])],
    );
    assert_eq!(source.discard_seq, 7);
    assert_eq!(source.cards.len(), 3, "the Embers are all gone");
    assert_eq!(source.pile(&spark).unwrap().quantity, "2");
    assert!(source.pile(&ember).is_none());
    // Ids stay the ids of the file.
    assert_eq!(source.ids, [p.ids[0].clone(), spark, p.ids[3].clone()]);
    assert!(source.has_product(1));
    assert!(!source.has_product(3));
}

#[tokio::test]
async fn the_service_parses_once_per_import_and_follows_discards() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&InventoryDb::in_dir(dir.path()), 1).unwrap();
    let service = StockService::default();
    assert!(service.source(&db).await.unwrap().is_none());

    db.write(|conn| Ok(keep_file(conn, &sample_csv())))
        .await
        .unwrap();
    let first = service.source(&db).await.unwrap().unwrap();
    let again = service.source(&db).await.unwrap().unwrap();
    assert!(Arc::ptr_eq(&first, &again));
    assert_eq!((first.discard_seq, first.cards.len()), (0, 4));

    // A pending discard of one Spark.
    let spark = first.ids[1].clone();
    let items = serde_json::to_string(&[item(&spark, 1)]).unwrap();
    db.write(move |conn| {
        conn.execute(
            "INSERT INTO srv_discards (category, file_version, created_at, user_name,
                                       copies, variants, items)
             VALUES ('Magic', 1, '2026-11-15T11:00:00Z', 'Aron', 1, 1, ?1)",
            [items],
        )
    })
    .await
    .unwrap();
    let after = service.source(&db).await.unwrap().unwrap();
    assert_eq!(after.discard_seq, 1);
    assert_eq!(after.pile(&spark).unwrap().quantity, "2");

    // Settled by the next import: the new file counts.
    db.write(|conn| {
        crate::discards::settle(conn, chrono::Utc::now())?;
        Ok(keep_file(
            conn,
            &(sample_csv() + &row(9, "New", 1, "A-0-0-9") + "\n"),
        ))
    })
    .await
    .unwrap();
    let newer = service.source(&db).await.unwrap().unwrap();
    assert_eq!((newer.version, newer.discard_seq), (2, 0));
    assert_eq!(newer.cards.len(), 5);
    assert_eq!(newer.pile(&spark).unwrap().quantity, "3");
}
