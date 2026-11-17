use super::*;

fn candidate(name: &str, sold: i64, listed: &str, sold_out: &str, price: f64) -> RestockCandidate {
    RestockCandidate {
        cardmarket_id: "1".to_string(),
        name: name.to_string(),
        set_code: "TST".to_string(),
        cn: "1".to_string(),
        condition: "NM".to_string(),
        language: "English".to_string(),
        is_foil: false,
        rarity: "Common".to_string(),
        sold_copies: sold,
        realized_revenue: sold as f64 * price,
        last_price: price,
        listed_date: listed.to_string(),
        sold_out_date: sold_out.to_string(),
        lot: Some("L1".to_string()),
    }
}

#[test]
fn velocity_derived_from_sell_through_window() {
    // 4 copies over 14 days → 2 copies/week.
    let ranked = rank_candidates(
        vec![candidate("Bolt", 4, "2026-01-01", "2026-01-15", 1.0)],
        1,
    );
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].days_to_sell_out, 14);
    assert!((ranked[0].copies_per_week - 2.0).abs() < 1e-9);
}

#[test]
fn same_day_sell_out_clamps_to_one_day() {
    let ranked = rank_candidates(
        vec![candidate("Bolt", 3, "2026-01-01", "2026-01-01", 1.0)],
        1,
    );
    assert_eq!(ranked[0].days_to_sell_out, 1);
    assert!((ranked[0].copies_per_week - 21.0).abs() < 1e-9);
}

#[test]
fn min_copies_filters_one_off_sales() {
    let ranked = rank_candidates(
        vec![
            candidate("One-off", 1, "2026-01-01", "2026-01-02", 1.0),
            candidate("Seller", 2, "2026-01-01", "2026-01-02", 1.0),
        ],
        2,
    );
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].candidate.name, "Seller");
}

#[test]
fn fastest_sellers_rank_first_revenue_breaks_ties() {
    let ranked = rank_candidates(
        vec![
            // 2 copies / 14 days = 1/week.
            candidate("Slow", 2, "2026-01-01", "2026-01-15", 5.0),
            // 4 copies / 7 days = 4/week.
            candidate("Fast", 4, "2026-01-01", "2026-01-08", 1.0),
            // Same velocity as Cheap-tie but higher revenue.
            candidate("Rich-tie", 2, "2026-01-01", "2026-01-08", 9.0),
            candidate("Cheap-tie", 2, "2026-01-01", "2026-01-08", 1.0),
        ],
        1,
    );
    let names: Vec<&str> = ranked.iter().map(|r| r.candidate.name.as_str()).collect();
    assert_eq!(names, ["Fast", "Rich-tie", "Cheap-tie", "Slow"]);
}

#[test]
fn timestamped_dates_parse_by_leading_day() {
    let ranked = rank_candidates(
        vec![candidate(
            "Bolt",
            2,
            "2026-01-01 09:30:00",
            "2026-01-08",
            1.0,
        )],
        1,
    );
    assert_eq!(ranked[0].days_to_sell_out, 7);
}

#[test]
fn buy_list_csv_has_header_and_rows_in_given_order() {
    let ranked = rank_candidates(
        vec![
            candidate("Fast", 4, "2026-01-01", "2026-01-08", 1.5),
            candidate("Slow", 2, "2026-01-01", "2026-01-15", 5.0),
        ],
        1,
    );
    let csv = format_buy_list_csv(&ranked, true);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("name,setCode,cn,condition,language,isFoil,rarity,lot,soldCopies"));
    assert!(lines[1].starts_with("Fast,TST,1,NM,English,,Common,L1,4,4.00,7,1.50,6.00"));
    assert!(lines[2].starts_with("Slow,TST,1,NM,English,,Common,L1,2,1.00,14,5.00,10.00"));
}

#[test]
fn buy_list_csv_empty_lot_stays_empty() {
    let mut c = candidate("Bolt", 2, "2026-01-01", "2026-01-08", 1.0);
    c.lot = None;
    let ranked = rank_candidates(vec![c], 1);
    let csv = format_buy_list_csv(&ranked, true);
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[1].starts_with("Bolt,TST,1,NM,English,,Common,,2,"));
}

#[test]
fn buy_list_csv_can_leave_out_revenue() {
    let ranked = rank_candidates(
        vec![candidate("Bolt", 2, "2026-01-01", "2026-01-08", 1.0)],
        1,
    );
    let csv = format_buy_list_csv(&ranked, false);
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].ends_with(",lastPrice"), "{}", lines[0]);
    assert!(!csv.contains("realizedRevenue"));
    assert!(lines[1].ends_with(",1.00"), "{}", lines[1]);
}

#[test]
fn buy_list_csv_text_cannot_become_a_formula() {
    let mut c = candidate(
        "=HYPERLINK(\"http://x\")",
        2,
        "2026-01-01",
        "2026-01-08",
        1.0,
    );
    c.set_code = "+cmd".to_string();
    let ranked = rank_candidates(vec![c], 1);
    let csv = format_buy_list_csv(&ranked, true);
    assert!(csv.contains("'=HYPERLINK"));
    assert!(csv.contains(",'+cmd,"));
}

#[test]
fn sorting_by_any_column_and_direction() {
    let mut rows = rank_candidates(
        vec![
            candidate("Fast", 4, "2026-01-01", "2026-01-08", 1.5),
            candidate("Slow", 2, "2026-01-01", "2026-01-15", 5.0),
            candidate("Mid", 3, "2026-01-01", "2026-01-11", 3.0),
        ],
        1,
    );
    let names = |rows: &[RankedRestock]| {
        rows.iter()
            .map(|r| r.candidate.name.clone())
            .collect::<Vec<_>>()
    };
    sort_ranked(&mut rows, RestockSort::Price, true);
    assert_eq!(names(&rows), ["Slow", "Mid", "Fast"]);
    sort_ranked(&mut rows, RestockSort::Price, false);
    assert_eq!(names(&rows), ["Fast", "Mid", "Slow"]);
    sort_ranked(&mut rows, RestockSort::Name, false);
    assert_eq!(names(&rows), ["Fast", "Mid", "Slow"]);
    sort_ranked(&mut rows, RestockSort::Velocity, true);
    assert_eq!(names(&rows), ["Fast", "Mid", "Slow"]);
    sort_ranked(&mut rows, RestockSort::Days, true);
    assert_eq!(names(&rows)[0], "Slow");
    sort_ranked(&mut rows, RestockSort::SoldCopies, true);
    assert_eq!(names(&rows)[0], "Fast");
    assert_eq!(RestockSort::default(), RestockSort::Velocity);
}
