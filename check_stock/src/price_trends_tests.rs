//! Tests for price_trends.

use super::*;

fn snap(id: u64, requested: &str, actual: &str, trend: Option<f64>) -> PriceSnapshot {
    PriceSnapshot {
        id_product: id,
        requested_date: requested.to_string(),
        price_date: actual.to_string(),
        avg: None,
        low: None,
        trend,
        avg1: None,
        avg7: None,
        avg30: None,
        avg_foil: None,
        low_foil: None,
        trend_foil: trend.map(|t| t * 2.0),
        avg1_foil: None,
        avg7_foil: None,
        avg30_foil: None,
    }
}

fn stock_card(id: &str, is_foil: bool, effective_date: &str) -> InStockCard {
    InStockCard {
        cardmarket_id: id.to_string(),
        name: "Test Card".to_string(),
        set_code: "tst".to_string(),
        cn: "1".to_string(),
        condition: "NM".to_string(),
        language: "English".to_string(),
        is_foil,
        rarity: "Rare".to_string(),
        quantity: 2,
        price: 1.0,
        location: "A1_S1_R1_C1".to_string(),
        effective_date: effective_date.to_string(),
    }
}

fn dates() -> SnapshotDates {
    [
        "2026-07-18".to_string(), // today
        "2026-07-11".to_string(), // −7d
        "2026-07-04".to_string(), // −14d
        "2026-06-18".to_string(), // −30d
        "2026-05-19".to_string(), // −60d
        "2026-04-19".to_string(), // −90d
    ]
}

#[test]
fn request_dates_follow_snapshot_days() {
    let today = NaiveDate::from_ymd_opt(2026, 7, 18).unwrap();
    let d = SnapshotSet::request_dates(today);
    assert_eq!(d, dates());
    assert_eq!(SNAPSHOT_SLOT_COUNT, SNAPSHOT_DAYS.len());
}

#[test]
fn pct_change_basic() {
    assert_eq!(pct_change(Some(2.0), Some(3.0)), Some(50.0));
    assert_eq!(pct_change(Some(2.0), Some(1.0)), Some(-50.0));
    assert_eq!(pct_change(None, Some(1.0)), None);
    assert_eq!(pct_change(Some(1.0), None), None);
    // Zero/negative base is unusable, not a division-by-zero panic.
    assert_eq!(pct_change(Some(0.0), Some(1.0)), None);
}

#[test]
fn change_computes_both_windows() {
    let d = dates();
    let set = SnapshotSet::new(
        &d,
        vec![
            snap(1, "2026-07-18", "2026-07-18", Some(2.0)),
            snap(1, "2026-07-11", "2026-07-11", Some(1.0)),
            snap(1, "2026-06-18", "2026-06-18", Some(4.0)),
        ],
    );
    let c = set.change(1, PriceField::Trend, false);
    assert_eq!(c.current, Some(2.0));
    assert_eq!(c.pct_7d, Some(100.0));
    assert_eq!(c.pct_30d, Some(-50.0));
}

#[test]
fn change_uses_foil_columns_for_foil() {
    let d = dates();
    let set = SnapshotSet::new(
        &d,
        vec![
            snap(1, "2026-07-18", "2026-07-18", Some(2.0)),
            snap(1, "2026-07-11", "2026-07-11", Some(1.0)),
        ],
    );
    let c = set.change(1, PriceField::Trend, true);
    // Foil trend is 2× the non-foil trend in the fixture; ratios are unchanged.
    assert_eq!(c.current, Some(4.0));
    assert_eq!(c.pct_7d, Some(100.0));
}

#[test]
fn change_is_none_when_same_underlying_row() {
    // Data collection started 2026-07-15: the "7 days ago" request resolves to
    // the same row as "today" — must be None, not a fake 0 %.
    let d = dates();
    let set = SnapshotSet::new(
        &d,
        vec![
            snap(1, "2026-07-18", "2026-07-15", Some(2.0)),
            snap(1, "2026-07-11", "2026-07-15", Some(2.0)),
        ],
    );
    let c = set.change(1, PriceField::Trend, false);
    assert_eq!(c.current, Some(2.0));
    assert_eq!(c.pct_7d, None);
    assert_eq!(c.pct_30d, None);
}

#[test]
fn change_for_unknown_product_is_default() {
    let set = SnapshotSet::new(&dates(), vec![]);
    assert_eq!(
        set.change(42, PriceField::Trend, false),
        TrendChange::default()
    );
    assert!(set.is_empty());
}

// ==================== volatility_pct ====================

#[test]
fn volatility_is_coefficient_of_variation() {
    let d = dates();
    // Four distinct rows with trend 2, 4, 6, 8 → mean 5, population σ ≈ 2.236.
    let set = SnapshotSet::new(
        &d,
        vec![
            snap(1, "2026-07-18", "2026-07-18", Some(2.0)),
            snap(1, "2026-07-11", "2026-07-11", Some(4.0)),
            snap(1, "2026-07-04", "2026-07-04", Some(6.0)),
            snap(1, "2026-06-18", "2026-06-18", Some(8.0)),
        ],
    );
    let v = set.volatility_pct(1, PriceField::Trend, false).unwrap();
    assert!((v - 44.721).abs() < 0.01);
    // Foil columns are 2× in the fixture → identical relative volatility.
    let vf = set.volatility_pct(1, PriceField::Trend, true).unwrap();
    assert!((vf - v).abs() < 0.001);
}

#[test]
fn volatility_dedupes_slots_resolving_to_same_row() {
    let d = dates();
    // Sparse history: every slot resolves to the same two underlying rows →
    // only 2 distinct values, not enough to judge volatility.
    let set = SnapshotSet::new(
        &d,
        vec![
            snap(1, "2026-07-18", "2026-07-15", Some(2.0)),
            snap(1, "2026-07-11", "2026-07-10", Some(4.0)),
            snap(1, "2026-07-04", "2026-07-10", Some(4.0)),
            snap(1, "2026-06-18", "2026-07-10", Some(4.0)),
        ],
    );
    assert_eq!(set.volatility_pct(1, PriceField::Trend, false), None);
}

#[test]
fn volatility_none_for_unknown_product_or_missing_values() {
    let d = dates();
    let set = SnapshotSet::new(
        &d,
        vec![
            snap(1, "2026-07-18", "2026-07-18", None),
            snap(1, "2026-07-11", "2026-07-11", None),
            snap(1, "2026-07-04", "2026-07-04", None),
        ],
    );
    assert_eq!(set.volatility_pct(1, PriceField::Trend, false), None);
    assert_eq!(set.volatility_pct(99, PriceField::Trend, false), None);
}

#[test]
fn volatility_zero_for_stable_prices() {
    let d = dates();
    let set = SnapshotSet::new(
        &d,
        vec![
            snap(1, "2026-07-18", "2026-07-18", Some(3.0)),
            snap(1, "2026-07-11", "2026-07-11", Some(3.0)),
            snap(1, "2026-06-18", "2026-06-18", Some(3.0)),
        ],
    );
    let v = set.volatility_pct(1, PriceField::Trend, false).unwrap();
    assert!(v.abs() < 1e-9);
}

#[test]
fn build_stock_movers_joins_and_filters() {
    let d = dates();
    let set = SnapshotSet::new(
        &d,
        vec![
            snap(1, "2026-07-18", "2026-07-18", Some(2.0)),
            snap(1, "2026-07-11", "2026-07-11", Some(1.0)),
            // Product 2: bulk-priced, below min_price.
            snap(2, "2026-07-18", "2026-07-18", Some(0.05)),
            snap(2, "2026-07-11", "2026-07-11", Some(0.10)),
            // Product 3: only one real row → no computable window.
            snap(3, "2026-07-18", "2026-07-15", Some(5.0)),
            snap(3, "2026-07-11", "2026-07-15", Some(5.0)),
        ],
    );
    let cards = vec![
        stock_card("1", false, "2026-05-18"),
        stock_card("2", false, "2026-07-01"),
        stock_card("3", false, "2026-07-01"),
        stock_card("not-a-number", false, "2026-07-01"),
        stock_card("999", false, "2026-07-01"), // no snapshots at all
    ];
    let today = NaiveDate::from_ymd_opt(2026, 7, 18).unwrap();
    let movers = build_stock_movers(&cards, &set, PriceField::Trend, today, 0.30);

    assert_eq!(movers.len(), 1);
    assert_eq!(movers[0].card.cardmarket_id, "1");
    assert_eq!(movers[0].change.pct_7d, Some(100.0));
    assert_eq!(movers[0].age_days, 61);
}

fn history_point(date: &str, trend: Option<f64>) -> PriceHistoryPoint {
    PriceHistoryPoint {
        price_date: date.to_string(),
        avg: None,
        low: None,
        trend,
        avg1: None,
        avg7: None,
        avg30: None,
        avg_foil: None,
        low_foil: None,
        trend_foil: trend.map(|t| t * 2.0),
        avg1_foil: None,
        avg7_foil: None,
        avg30_foil: None,
    }
}

#[test]
fn roc_from_history_picks_baseline_at_or_before_cutoff() {
    let history = vec![
        history_point("2026-06-18", Some(4.0)),
        history_point("2026-07-10", Some(1.0)),
        history_point("2026-07-11", Some(2.0)),
        history_point("2026-07-18", Some(3.0)),
    ];
    // 7 days before 07-18 is 07-11 → baseline 2.0, current 3.0.
    assert_eq!(roc_from_history(&history, 7, false), Some(50.0));
    // 30 days back → 06-18 baseline 4.0.
    assert_eq!(roc_from_history(&history, 30, false), Some(-25.0));
    // Foil columns are 2× in the fixture; the ratio is identical.
    assert_eq!(roc_from_history(&history, 7, true), Some(50.0));
}

#[test]
fn roc_from_history_none_when_history_too_short() {
    let history = vec![
        history_point("2026-07-17", Some(1.0)),
        history_point("2026-07-18", Some(2.0)),
    ];
    assert_eq!(roc_from_history(&history, 7, false), None);
    assert_eq!(roc_from_history(&[], 7, false), None);
}

#[test]
fn roc_from_history_skips_rows_without_values() {
    let history = vec![
        history_point("2026-07-01", Some(2.0)),
        history_point("2026-07-11", None), // at the cutoff but no value
        history_point("2026-07-18", Some(3.0)),
    ];
    // Falls back to the older 07-01 row for the baseline.
    assert_eq!(roc_from_history(&history, 7, false), Some(50.0));
}
