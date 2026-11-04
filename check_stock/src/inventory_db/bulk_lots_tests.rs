//! Tests for bulk lots: recording, the pending-removal ledger the sync uses up,
//! and how lot revenue reaches the reports.

use super::super::{
    get_db_stats_conn, get_restock_candidates_conn, get_sold_events_conn, init_schema,
    lot_breakdown_from, preview_sync_conn, read_snapshots_conn, sync_inventory_conn,
    visit_digest_conn, SyncStats,
};
use super::*;
use crate::category::DEFAULT_CATEGORY as CAT;

fn test_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    conn
}

/// An inventory CSV row: `qty` copies of product `id` at `location`.
fn card(id: &str, qty: i64, price: &str, location: &str) -> Card {
    Card {
        cardmarket_id: id.to_string(),
        quantity: qty.to_string(),
        name: format!("Card {id}"),
        set: "Test Set".to_string(),
        set_code: "TST".to_string(),
        cn: id.to_string(),
        condition: "NM".to_string(),
        language: "English".to_string(),
        is_foil: String::new(),
        finish_type: "Regular".to_string(),
        is_playset: None,
        is_signed: String::new(),
        is_first_ed: None,
        is_reverse_holo: None,
        price: price.to_string(),
        comment: String::new(),
        location: Some(location.to_string()),
        name_de: String::new(),
        name_es: String::new(),
        name_fr: String::new(),
        name_it: String::new(),
        rarity: "Common".to_string(),
        listed_at: String::new(),
    }
}

fn sync(conn: &mut Connection, cards: &[Card], date: &str) -> SyncStats {
    sync_inventory_conn(conn, cards, CAT, date).unwrap()
}

fn record(conn: &mut Connection, price: f64, lines: &[(Card, i64)], date: &str) -> i64 {
    record_bulk_lot_conn(conn, "100 Commons", price, lines, CAT, date).unwrap()
}

/// `(quantity, sold_quantity)` of product `id`.
fn stock(conn: &Connection, id: &str) -> (i64, i64) {
    conn.query_row(
        "SELECT quantity, sold_quantity FROM inventory_cards WHERE cardmarket_id = ?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

fn pending(conn: &Connection, lot_id: i64) -> i64 {
    list_bulk_lots_conn(conn)
        .unwrap()
        .into_iter()
        .find(|l| l.id == lot_id)
        .unwrap()
        .pending_copies
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ── Recording ────────────────────────────────────────────────────────────────

#[test]
fn record_lists_the_lot_with_all_copies_pending() {
    let mut conn = test_conn();
    let a = card("1", 3, "0.10", "A-0-0-1-L1-R");
    let b = card("2", 5, "0.20", "A-0-0-2-L2-R");
    let id = record(&mut conn, 1.5, &[(a, 2), (b, 1)], "2026-11-04");

    let lots = list_bulk_lots_conn(&conn).unwrap();
    assert_eq!(lots.len(), 1);
    let lot = &lots[0];
    assert_eq!(lot.id, id);
    assert_eq!(lot.name, "100 Commons");
    assert_eq!(lot.created_at, "2026-11-04");
    assert_eq!(lot.copies, 3);
    assert_eq!(lot.pending_copies, 3);
    assert!(approx(lot.listed_value, 0.4), "2 × 0.10 + 1 × 0.20");
    assert!(approx(lot.per_card(), 0.5));
}

#[test]
fn record_stores_the_cards_with_their_purchase_lot() {
    let mut conn = test_conn();
    let mut foil = card("1", 3, "0,10", "A-0-0-1-L7-R");
    foil.finish_type = "Foil".to_string();
    let id = record(&mut conn, 1.0, &[(foil, 2)], "2026-11-04");

    let cards = bulk_lot_cards_conn(&conn, id).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].purchase_lot, "L7");
    assert_eq!(cards[0].location, "A-0-0-1-L7-R");
    assert_eq!(cards[0].copies, 2);
    assert!(cards[0].foil, "finishType foil is shown as foil");
    assert!(approx(cards[0].listed_price, 0.10), "comma decimal parsed");
}

#[test]
fn record_rejects_invalid_lots_without_writing() {
    let mut conn = test_conn();
    let c = card("1", 3, "0.10", "A-0-0-1-L1-R");
    let ok = [(c.clone(), 1)];
    let too_long = "x".repeat(MAX_LOT_NAME_CHARS + 1);
    // (name, price, lines) of a lot that must be refused.
    type Attempt<'a> = (&'a str, f64, Vec<(Card, i64)>);
    let bad: Vec<Attempt> = vec![
        ("   ", 1.0, ok.to_vec()),
        (&too_long, 1.0, ok.to_vec()),
        ("Lot", -0.01, ok.to_vec()),
        ("Lot", f64::NAN, ok.to_vec()),
        ("Lot", MAX_LOT_PRICE_EUR + 1.0, ok.to_vec()),
        ("Lot", 1.0, vec![]),
        ("Lot", 1.0, vec![(c.clone(), 0)]),
        ("Lot", 1.0, vec![(c.clone(), MAX_LOT_COPIES + 1)]),
    ];
    for (name, price, lines) in bad {
        assert!(
            record_bulk_lot_conn(&mut conn, name, price, &lines, CAT, "2026-11-04").is_err(),
            "{name:?} / {price} / {} lines must be rejected",
            lines.len()
        );
    }
    assert!(list_bulk_lots_conn(&conn).unwrap().is_empty());
}

#[test]
fn record_trims_the_name_and_allows_a_free_lot() {
    let mut conn = test_conn();
    let c = card("1", 3, "0.10", "A-0-0-1-L1-R");
    record_bulk_lot_conn(&mut conn, "  Freebies ", 0.0, &[(c, 1)], CAT, "2026-11-04").unwrap();
    assert_eq!(list_bulk_lots_conn(&conn).unwrap()[0].name, "Freebies");
}

// ── Sync: removals are not sales ─────────────────────────────────────────────

#[test]
fn import_then_sync_removes_the_copies_without_a_sale() {
    let mut conn = test_conn();
    let c = card("1", 10, "0.10", "A-0-0-1-L1-R");
    sync(&mut conn, std::slice::from_ref(&c), "2026-11-01");
    let id = record(&mut conn, 1.0, &[(c.clone(), 4)], "2026-11-02");

    let after_import = card("1", 6, "0.10", "A-0-0-1-L1-R");
    let stats = sync(&mut conn, &[after_import], "2026-11-03");

    assert_eq!(stats.bulk_removed, 4);
    assert_eq!(stock(&conn, "1"), (6, 0), "removed, not sold");
    assert!(get_sold_events_conn(&conn, CAT).unwrap().is_empty());
    assert_eq!(pending(&conn, id), 0);
}

#[test]
fn sync_before_the_import_keeps_the_removal_pending() {
    let mut conn = test_conn();
    let c = card("1", 10, "0.10", "A-0-0-1-L1-R");
    sync(&mut conn, std::slice::from_ref(&c), "2026-11-01");
    let id = record(&mut conn, 1.0, &[(c.clone(), 4)], "2026-11-02");

    // An export taken before the Powertools import still lists all 10.
    let stats = sync(&mut conn, std::slice::from_ref(&c), "2026-11-03");
    assert_eq!(stats.bulk_removed, 0);
    assert_eq!(pending(&conn, id), 4);

    // The next export after the import: the drop is the lot, not a sale.
    sync(
        &mut conn,
        &[card("1", 6, "0.10", "A-0-0-1-L1-R")],
        "2026-11-05",
    );
    assert_eq!(stock(&conn, "1"), (6, 0));
    assert_eq!(pending(&conn, id), 0);
    assert!(get_sold_events_conn(&conn, CAT).unwrap().is_empty());
}

#[test]
fn a_single_sale_in_the_same_sync_is_still_recorded() {
    let mut conn = test_conn();
    let c = card("1", 10, "0.30", "A-0-0-1-L1-R");
    sync(&mut conn, std::slice::from_ref(&c), "2026-11-01");
    record(&mut conn, 1.0, &[(c.clone(), 4)], "2026-11-02");

    // 4 left in the lot, 1 sold as a single.
    let stats = sync(
        &mut conn,
        &[card("1", 5, "0.30", "A-0-0-1-L1-R")],
        "2026-11-03",
    );
    assert_eq!(stats.bulk_removed, 4);
    assert_eq!(stock(&conn, "1"), (5, 1));
    let events = get_sold_events_conn(&conn, CAT).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].copies, 1);
    assert!(approx(events[0].price, 0.30));
}

#[test]
fn a_smaller_drop_uses_up_only_part_of_the_removal() {
    let mut conn = test_conn();
    let c = card("1", 10, "0.10", "A-0-0-1-L1-R");
    sync(&mut conn, std::slice::from_ref(&c), "2026-11-01");
    let id = record(&mut conn, 1.0, &[(c.clone(), 4)], "2026-11-02");

    let stats = sync(
        &mut conn,
        &[card("1", 8, "0.10", "A-0-0-1-L1-R")],
        "2026-11-03",
    );
    assert_eq!(stats.bulk_removed, 2);
    assert_eq!(stock(&conn, "1"), (8, 0));
    assert_eq!(pending(&conn, id), 2, "two copies still to be seen gone");
}

#[test]
fn a_variant_gone_from_the_csv_into_a_lot_is_not_sold() {
    let mut conn = test_conn();
    let keep = card("1", 5, "0.10", "A-0-0-1-L1-R");
    let dumped = card("2", 3, "0.10", "A-0-0-2-L1-R");
    sync(&mut conn, &[keep.clone(), dumped.clone()], "2026-11-01");
    record(&mut conn, 1.0, &[(dumped, 3)], "2026-11-02");

    let stats = sync(&mut conn, &[keep], "2026-11-03");
    assert_eq!(stats.zeroed, 1);
    assert_eq!(stats.bulk_removed, 3);
    assert_eq!(stock(&conn, "2"), (0, 0));
    assert!(get_sold_events_conn(&conn, CAT).unwrap().is_empty());
}

#[test]
fn a_vanished_variant_partly_in_a_lot_sells_the_rest() {
    let mut conn = test_conn();
    let keep = card("1", 5, "0.10", "A-0-0-1-L1-R");
    let partly = card("2", 3, "0.40", "A-0-0-2-L1-R");
    sync(&mut conn, &[keep.clone(), partly.clone()], "2026-11-01");
    record(&mut conn, 1.0, &[(partly, 1)], "2026-11-02");

    sync(&mut conn, &[keep], "2026-11-03");
    assert_eq!(stock(&conn, "2"), (0, 2));
    let events = get_sold_events_conn(&conn, CAT).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].copies, 2);
}

#[test]
fn removals_from_two_lots_of_one_card_are_used_oldest_first() {
    let mut conn = test_conn();
    let c = card("1", 10, "0.10", "A-0-0-1-L1-R");
    sync(&mut conn, std::slice::from_ref(&c), "2026-11-01");
    let first = record(&mut conn, 1.0, &[(c.clone(), 2)], "2026-11-02");
    let second = record(&mut conn, 1.0, &[(c.clone(), 3)], "2026-11-02");

    sync(
        &mut conn,
        &[card("1", 7, "0.10", "A-0-0-1-L1-R")],
        "2026-11-03",
    );
    assert_eq!(pending(&conn, first), 0);
    assert_eq!(pending(&conn, second), 2);
    assert_eq!(stock(&conn, "1"), (7, 0));
}

#[test]
fn removals_only_apply_to_their_own_category() {
    let mut conn = test_conn();
    let c = card("1", 10, "0.10", "A-0-0-1-L1-R");
    sync(&mut conn, std::slice::from_ref(&c), "2026-11-01");
    sync_inventory_conn(&mut conn, std::slice::from_ref(&c), "Generic", "2026-11-01").unwrap();
    let id = record(&mut conn, 1.0, &[(c, 4)], "2026-11-02");

    // The same product id in another category dropping is a sale there.
    sync_inventory_conn(
        &mut conn,
        &[card("1", 6, "0.10", "A-0-0-1-L1-R")],
        "Generic",
        "2026-11-03",
    )
    .unwrap();
    assert_eq!(pending(&conn, id), 4);
    assert_eq!(get_sold_events_conn(&conn, "Generic").unwrap()[0].copies, 4);
}

// ── Import preview and safety guard ──────────────────────────────────────────

#[test]
fn preview_does_not_count_lot_copies_as_sold() {
    let mut conn = test_conn();
    let c = card("1", 10, "0.10", "A-0-0-1-L1-R");
    sync(&mut conn, std::slice::from_ref(&c), "2026-11-01");
    record(&mut conn, 1.0, &[(c, 4)], "2026-11-02");

    let preview = preview_sync_conn(&conn, &[card("1", 5, "0.10", "A-0-0-1-L1-R")], CAT).unwrap();
    assert_eq!(preview.bulk_removed, 4);
    assert_eq!(preview.copies_sold, 1);
}

#[test]
fn a_large_lot_does_not_trip_the_import_guard() {
    let mut conn = test_conn();
    let c = card("1", 200, "0.05", "A-0-0-1-L1-R");
    sync(&mut conn, std::slice::from_ref(&c), "2026-11-01");
    record(&mut conn, 2.0, &[(c, 150)], "2026-11-02");

    let preview = preview_sync_conn(&conn, &[card("1", 50, "0.05", "A-0-0-1-L1-R")], CAT).unwrap();
    assert_eq!(preview.copies_sold, 0);
    assert!(!preview.is_suspicious());
}

// ── Purchase lots ────────────────────────────────────────────────────────────

#[test]
fn lot_price_is_credited_to_purchase_lots_per_copy() {
    let mut conn = test_conn();
    let a = card("1", 5, "0.10", "A-0-0-1-L1-R");
    let b = card("2", 5, "0.10", "A-0-0-2-L2-R");
    sync(&mut conn, &[a.clone(), b.clone()], "2026-11-01");
    record(&mut conn, 1.0, &[(a, 3), (b, 1)], "2026-11-02");

    let lots = lot_breakdown_from(&conn).unwrap();
    let l1 = lots.iter().find(|l| l.lot == "L1").unwrap();
    let l2 = lots.iter().find(|l| l.lot == "L2").unwrap();
    assert_eq!((l1.bulk_copies, l1.sold_copies), (3, 3));
    assert!(approx(l1.bulk_revenue, 0.75) && approx(l1.sold_revenue, 0.75));
    assert_eq!(l2.bulk_copies, 1);
    assert!(approx(l2.sold_revenue, 0.25));
}

#[test]
fn a_purchase_lot_known_only_from_a_bulk_lot_still_appears() {
    let mut conn = test_conn();
    let c = card("1", 2, "0.10", "A-0-0-1-L9-R");
    record(&mut conn, 0.5, &[(c, 2)], "2026-11-02");
    let lots = lot_breakdown_from(&conn).unwrap();
    assert_eq!(lots.len(), 1);
    assert_eq!(lots[0].lot, "L9");
    assert!(approx(lots[0].sold_revenue, 0.5));
}

#[test]
fn removal_sync_credits_the_lot_price_not_the_listed_price() {
    let mut conn = test_conn();
    let c = card("1", 4, "2.00", "A-0-0-1-L1-R");
    sync(&mut conn, std::slice::from_ref(&c), "2026-11-01");
    record(&mut conn, 0.4, &[(c, 4)], "2026-11-02");
    sync(&mut conn, &[], "2026-11-03");

    let lots = lot_breakdown_from(&conn).unwrap();
    assert_eq!(lots[0].sold_copies, 4);
    assert!(
        approx(lots[0].sold_revenue, 0.4),
        "{}",
        lots[0].sold_revenue
    );
}

#[test]
fn a_single_sale_is_credited_to_the_lot_it_left_not_the_lot_the_bulk_left() {
    let mut conn = test_conn();
    sync(
        &mut conn,
        &[
            card("1", 3, "1.00", "A-0-0-1-L1-R"),
            card("1", 3, "1.00", "A-0-0-2-L2-R"),
        ],
        "2026-11-01",
    );
    // Two copies from L2 go into a lot; one copy from L1 sells as a single.
    record(
        &mut conn,
        0.2,
        &[(card("1", 3, "1.00", "A-0-0-2-L2-R"), 2)],
        "2026-11-02",
    );
    sync(
        &mut conn,
        &[
            card("1", 2, "1.00", "A-0-0-1-L1-R"),
            card("1", 1, "1.00", "A-0-0-2-L2-R"),
        ],
        "2026-11-03",
    );

    let lots = lot_breakdown_from(&conn).unwrap();
    let l1 = lots.iter().find(|l| l.lot == "L1").unwrap();
    let l2 = lots.iter().find(|l| l.lot == "L2").unwrap();
    assert_eq!(l1.sold_copies, 1);
    assert!(approx(l1.sold_revenue, 1.0));
    assert_eq!((l2.sold_copies, l2.bulk_copies), (2, 2));
    assert!(approx(l2.sold_revenue, 0.2));
}

// ── Revenue reports ──────────────────────────────────────────────────────────

#[test]
fn snapshot_revenue_includes_the_lot_price_but_not_its_copies() {
    let mut conn = test_conn();
    let c = card("1", 10, "1.00", "A-0-0-1-L1-R");
    sync(&mut conn, std::slice::from_ref(&c), "2026-11-01");
    record(&mut conn, 3.0, &[(c, 5)], "2026-11-02");
    sync(
        &mut conn,
        &[card("1", 5, "1.00", "A-0-0-1-L1-R")],
        "2026-11-03",
    );

    let snaps = read_snapshots_conn(&conn).unwrap();
    let last = snaps.last().unwrap();
    assert_eq!(last.sold_copies_cumulative, 0);
    assert!(approx(last.sold_revenue_cumulative, 3.0));
    assert!(
        approx(snaps[0].sold_revenue_cumulative, 0.0),
        "built after day 1"
    );
}

#[test]
fn velocity_reports_the_bulk_lots_its_revenue_includes() {
    let mut conn = test_conn();
    let c = card("1", 10, "1.00", "A-0-0-1-L1-R");
    sync(&mut conn, std::slice::from_ref(&c), "2026-11-01");
    record(&mut conn, 3.0, &[(c.clone(), 5)], "2026-11-02");
    sync(&mut conn, std::slice::from_ref(&c), "2026-11-09");
    // Built after the latest snapshot: not in velocity yet, but in the totals.
    record(&mut conn, 2.0, &[(c, 1)], "2026-11-10");

    let stats = get_db_stats_conn(&conn, CAT, "2026-11-10").unwrap();
    let v = stats.velocity.unwrap();
    assert!(approx(v.sold_revenue, 3.0));
    assert_eq!(v.bulk_lots.lots, 1);
    assert_eq!(v.bulk_lots.copies, 5);
    assert_eq!(stats.bulk_totals.lots, 2);
    assert!(approx(stats.bulk_totals.revenue, 5.0));
}

#[test]
fn digest_lists_bulk_lots_built_since_the_last_visit() {
    let mut conn = test_conn();
    visit_digest_conn(&conn, "2026-11-01").unwrap();
    let c = card("1", 10, "1.00", "A-0-0-1-L1-R");
    record(&mut conn, 1.0, &[(c.clone(), 3)], "2026-11-01");
    record(&mut conn, 2.0, &[(c, 2)], "2026-11-02");

    let digest = visit_digest_conn(&conn, "2026-11-02").unwrap();
    assert_eq!(
        digest.bulk_lots.lots, 1,
        "only the lot built after the baseline"
    );
    assert_eq!(digest.bulk_lots.copies, 2);
    assert!(approx(digest.bulk_lots.revenue, 2.0));
    assert_eq!(digest.sold_copies, 0, "lots are not single sales");
}

#[test]
fn variants_dumped_into_a_lot_are_not_restock_candidates() {
    let mut conn = test_conn();
    let sold = card("1", 2, "1.00", "A-0-0-1-L1-R");
    let dumped = card("2", 2, "1.00", "A-0-0-2-L1-R");
    sync(&mut conn, &[sold.clone(), dumped.clone()], "2026-11-01");
    // One copy of each sells as a single, then the last copy of #2 is dumped.
    sync(
        &mut conn,
        &[
            card("1", 1, "1.00", "A-0-0-1-L1-R"),
            card("2", 1, "1.00", "A-0-0-2-L1-R"),
        ],
        "2026-11-02",
    );
    record(&mut conn, 0.1, &[(dumped, 1)], "2026-11-03");
    sync(&mut conn, &[], "2026-11-04");

    let ids: Vec<String> = get_restock_candidates_conn(&conn, Some(CAT))
        .unwrap()
        .into_iter()
        .map(|c| c.cardmarket_id)
        .collect();
    assert_eq!(ids, vec!["1".to_string()]);
    let digest = visit_digest_conn(&conn, "2026-11-05").unwrap();
    assert_eq!(digest.restock_candidates, 1);
}

// ── Editing the history ──────────────────────────────────────────────────────

#[test]
fn changing_the_price_changes_the_revenue_split() {
    let mut conn = test_conn();
    let id = record(
        &mut conn,
        1.0,
        &[(card("1", 4, "0.10", "A-0-0-1-L1-R"), 4)],
        "2026-11-02",
    );
    set_bulk_lot_price_conn(&conn, id, 2.0).unwrap();
    assert!(approx(
        list_bulk_lots_conn(&conn).unwrap()[0].per_card(),
        0.5
    ));
    assert!(approx(
        lot_breakdown_from(&conn).unwrap()[0].bulk_revenue,
        2.0
    ));

    assert!(set_bulk_lot_price_conn(&conn, id, -1.0).is_err());
    assert!(
        set_bulk_lot_price_conn(&conn, id + 1, 1.0).is_err(),
        "unknown lot"
    );
}

#[test]
fn mark_removed_clears_the_pending_copies() {
    let mut conn = test_conn();
    let c = card("1", 10, "0.10", "A-0-0-1-L1-R");
    sync(&mut conn, std::slice::from_ref(&c), "2026-11-01");
    let id = record(&mut conn, 1.0, &[(c, 4)], "2026-11-02");
    mark_bulk_lot_removed_conn(&conn, id).unwrap();
    assert_eq!(pending(&conn, id), 0);
    assert!(pending_removals_conn(&conn, CAT).unwrap().is_empty());

    // A later drop is now an ordinary sale.
    sync(
        &mut conn,
        &[card("1", 9, "0.10", "A-0-0-1-L1-R")],
        "2026-11-03",
    );
    assert_eq!(stock(&conn, "1"), (9, 1));
}

#[test]
fn deleting_a_lot_removes_its_revenue_and_pending_removals() {
    let mut conn = test_conn();
    let c = card("1", 10, "0.10", "A-0-0-1-L1-R");
    let id = record(&mut conn, 1.0, &[(c, 4)], "2026-11-02");
    delete_bulk_lot_conn(&mut conn, id).unwrap();

    assert!(list_bulk_lots_conn(&conn).unwrap().is_empty());
    assert!(pending_removals_conn(&conn, CAT).unwrap().is_empty());
    assert!(lot_breakdown_from(&conn).unwrap().is_empty());
    assert!(delete_bulk_lot_conn(&mut conn, id).is_err(), "already gone");
}

// ── Reads for the lot builder ────────────────────────────────────────────────

#[test]
fn pending_removals_carry_the_variant_key_of_the_sync() {
    let mut conn = test_conn();
    let mut c = card("1", 3, "0.10", "A-0-0-1-L4-R");
    c.condition = "near_mint".to_string();
    c.language = "en".to_string();
    c.is_foil = "false".to_string();
    record(&mut conn, 1.0, &[(c.clone(), 2)], "2026-11-02");

    let removals = pending_removals_conn(&conn, CAT).unwrap();
    assert_eq!(removals.len(), 1);
    assert_eq!(removals[0].variant, super::super::card_variant_key(&c, CAT));
    assert_eq!(removals[0].purchase_lot, "L4");
    assert_eq!(removals[0].pending, 2);
}

#[test]
fn listing_dates_fall_back_to_the_first_sync() {
    let mut conn = test_conn();
    let undated = card("1", 3, "0.10", "A-0-0-1-L1-R");
    let mut dated = card("2", 3, "0.10", "A-0-0-2-L1-R");
    dated.listed_at = "2025-06-01".to_string();
    sync(&mut conn, &[undated.clone(), dated.clone()], "2026-02-24");

    let dates = listing_dates_conn(&conn, CAT).unwrap();
    let key = |c: &Card| super::super::card_variant_key(c, CAT);
    assert_eq!(dates[&key(&undated)], "2026-02-24");
    assert_eq!(dates[&key(&dated)], "2025-06-01");
}

// ── RemovalPlan ──────────────────────────────────────────────────────────────

fn removal(id: i64, lot_id: i64, variant: &str, lot: &str, copies: i64) -> PendingRemoval {
    PendingRemoval {
        id,
        bulk_lot_id: lot_id,
        variant: variant.to_string(),
        purchase_lot: lot.to_string(),
        location: String::new(),
        pending: copies,
    }
}

#[test]
fn plan_is_capped_by_the_drop_and_splits_by_purchase_lot() {
    let drops = HashMap::from([("v".to_string(), 5)]);
    let pending = [
        removal(1, 1, "v", "L1", 3),
        removal(2, 1, "v", "L2", 4),
        removal(3, 2, "other", "L1", 2),
    ];
    let plan = RemovalPlan::plan(&drops, &pending);
    assert_eq!(plan.removed("v"), 5);
    assert_eq!(plan.removed("other"), 0, "no drop, stays pending");
    assert_eq!(plan.total(), 5);
    let by_lot = plan.removed_by_lot("v").unwrap();
    assert_eq!(by_lot["L1"], 3);
    assert_eq!(by_lot["L2"], 2);
    assert_eq!(plan.consumed, vec![(1, 3), (2, 2)]);
}

#[test]
fn plan_without_pending_removals_is_empty() {
    let drops = HashMap::from([("v".to_string(), 5)]);
    let plan = RemovalPlan::plan(&drops, &[]);
    assert_eq!(plan.total(), 0);
    assert!(plan.removed_by_lot("v").is_none());
}

#[test]
fn per_card_share_of_an_empty_lot_is_zero() {
    assert!(approx(per_card_share(1.0, 0), 0.0));
    assert!(approx(per_card_share(1.0, 100), 0.01));
}
