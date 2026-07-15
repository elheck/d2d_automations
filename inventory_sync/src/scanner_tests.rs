//! Tests for scanner.

use super::*;

fn point(date: &str, low: f64, trend: f64, avg1: f64, avg7: f64) -> PriceHistoryPoint {
    PriceHistoryPoint {
        price_date: date.to_string(),
        avg: Some(trend),
        low: Some(low),
        trend: Some(trend),
        avg1: Some(avg1),
        avg7: Some(avg7),
        avg30: Some(trend),
        avg_foil: None,
        low_foil: None,
        trend_foil: None,
        avg1_foil: None,
        avg7_foil: None,
        avg30_foil: None,
    }
}

/// Build a series that declines steadily then bottoms out — a classic dip.
fn declining_series(id: u64, name: &str) -> ProductPriceSeries {
    let mut points = Vec::new();
    // 40 days: price falls from 20.0 down to ~8.0, low tracks 70% of trend.
    for i in 0..40 {
        let trend = 20.0 - (i as f64) * 0.3;
        let trend = trend.max(8.0);
        let low = trend * 0.7; // heavy undercutting → strong floor signal
        let date = format!("2026-01-{:02}", i + 1);
        points.push(point(&date, low, trend, trend * 0.99, trend * 1.02));
    }
    ProductPriceSeries {
        id_product: id,
        name: name.to_string(),
        category_name: "Magic Single".to_string(),
        id_expansion: 1,
        expansion_name: Some("Alpha".to_string()),
        points,
    }
}

/// Build a series that rises steadily — should score low as a dip-buy.
fn rising_series(id: u64, name: &str) -> ProductPriceSeries {
    let mut points = Vec::new();
    for i in 0..40 {
        let trend = 8.0 + (i as f64) * 0.3;
        let low = trend * 0.98; // listings near trend, no undercutting
        let date = format!("2026-01-{:02}", i + 1);
        points.push(point(&date, low, trend, trend * 1.01, trend * 0.98));
    }
    ProductPriceSeries {
        id_product: id,
        name: name.to_string(),
        category_name: "Magic Single".to_string(),
        id_expansion: 1,
        expansion_name: None,
        points,
    }
}

#[test]
fn short_series_is_skipped() {
    let mut s = declining_series(1, "Shorty");
    s.points.truncate(MIN_POINTS - 1);
    assert!(score_series(&s).is_none());
}

#[test]
fn declining_card_scores_higher_than_rising_card() {
    let dip = score_series(&declining_series(1, "Dip")).unwrap();
    let rise = score_series(&rising_series(2, "Rise"));

    assert!(dip.score > 0.0);
    // A steadily rising card at trend is not an undervalued dip-buy.
    let rise_score = rise.map(|r| r.score).unwrap_or(0.0);
    assert!(
        dip.score > rise_score,
        "dip {} should beat rise {}",
        dip.score,
        rise_score
    );
}

#[test]
fn declining_card_has_floor_reason() {
    let dip = score_series(&declining_series(1, "Dip")).unwrap();
    assert!(!dip.reasons.is_empty());
    assert!(
        dip.reasons.iter().any(|r| r.contains("below trend")),
        "expected a floor reason, got {:?}",
        dip.reasons
    );
    // floor_ratio ~0.7 should be reported.
    assert!(dip.floor_ratio.unwrap() < 0.8);
}

#[test]
fn scan_sorts_by_score_descending_and_respects_limit() {
    let series = vec![rising_series(1, "Rise"), declining_series(2, "Dip")];
    let results = scan_buy_signals(&series, 10);
    // Dip should rank first (or rising filtered out entirely).
    assert!(!results.is_empty());
    assert_eq!(results[0].name, "Dip");

    // Limit is respected.
    let limited = scan_buy_signals(&series, 1);
    assert_eq!(limited.len(), 1);
}

#[test]
fn zero_score_candidates_are_dropped() {
    // A perfectly flat, at-trend card has no dip-buy signal.
    let mut points = Vec::new();
    for i in 0..40 {
        let date = format!("2026-01-{:02}", i + 1);
        points.push(point(&date, 10.0, 10.0, 10.0, 10.0));
    }
    let flat = ProductPriceSeries {
        id_product: 1,
        name: "Flat".to_string(),
        category_name: "Magic Single".to_string(),
        id_expansion: 1,
        expansion_name: None,
        points,
    };
    let results = scan_buy_signals(&[flat], 10);
    assert!(
        results.is_empty(),
        "flat at-trend card should score 0 and be dropped"
    );
}

#[test]
fn sub_scores_clamp_to_unit_range() {
    // Extreme undervaluation shouldn't exceed 1.0 per sub-score.
    let sub = compute_sub_scores(Some(0.2), Some(5.0), Some(-0.3), Some(-80.0), Some(2.0));
    assert!((0.0..=1.0).contains(&sub.floor));
    assert!((0.0..=1.0).contains(&sub.rsi));
    assert!((0.0..=1.0).contains(&sub.position));
    assert!((0.0..=1.0).contains(&sub.dip));
    assert!((0.0..=1.0).contains(&sub.bounce));
}

#[test]
fn implausible_undercut_is_treated_as_noise() {
    // low = 2% of trend is almost certainly a mispriced/damaged single, not a buy.
    let noisy = compute_sub_scores(Some(0.02), None, None, None, None);
    assert_eq!(
        noisy.floor, 0.0,
        "sub-30% ratios should be discarded as noise"
    );

    // A healthy 25% undercut is the sweet spot.
    let healthy = compute_sub_scores(Some(0.75), None, None, None, None);
    assert!(
        healthy.floor > 0.9,
        "25% undercut should be a strong floor signal"
    );
}

#[test]
fn floor_only_signal_is_dampened_without_corroboration() {
    // Build a series where the only signal is a floor undercut: flat trend
    // (no dip, RSI ~50, mid-band) but low sitting at a healthy undercut.
    let mut points = Vec::new();
    for i in 0..40 {
        let date = format!("2026-01-{:02}", i + 1);
        // trend flat at 10, low at 7.5 (0.75 ratio), avg1==avg7 (no bounce).
        points.push(point(&date, 7.5, 10.0, 10.0, 10.0));
    }
    let floor_only = ProductPriceSeries {
        id_product: 1,
        name: "FloorOnly".to_string(),
        category_name: "Magic Single".to_string(),
        id_expansion: 1,
        expansion_name: None,
        points,
    };
    let signal = score_series(&floor_only).unwrap();
    // Floor sub-score ~1.0 * 0.28 weight = 0.28 → *0.5 dampening → ~14, not ~28.
    assert!(
        signal.score < 20.0,
        "uncorroborated floor-only card should be dampened, got {}",
        signal.score
    );
}

#[test]
fn missing_indicators_contribute_zero() {
    let sub = compute_sub_scores(None, None, None, None, None);
    assert_eq!(sub.floor, 0.0);
    assert_eq!(sub.rsi, 0.0);
    assert_eq!(sub.position, 0.0);
    assert_eq!(sub.dip, 0.0);
    assert_eq!(sub.bounce, 0.0);
}

#[test]
fn scan_since_date_subtracts_window() {
    let since = scan_since_date("2026-05-01");
    // 120 days before 2026-05-01 is 2026-01-01.
    assert_eq!(since, "2026-01-01");
}

#[test]
fn scan_since_date_falls_back_on_bad_date() {
    assert_eq!(scan_since_date("not-a-date"), "0000-01-01");
}

#[test]
fn run_scan_persists_ranked_results() {
    use crate::cardmarket::{make_test_price_entry, make_test_product, PriceGuide, ProductCatalog};
    use crate::database::{get_buy_signals, init_schema, insert_price_history, upsert_products};

    let mut conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();

    // A card that declines steadily — should surface as a dip-buy.
    let catalog = ProductCatalog::from_entries(vec![make_test_product(1, "Dipper")]);
    upsert_products(&mut conn, &catalog).unwrap();

    // 40 days of declining trend; low tracks 80% of trend (make_test_price_entry).
    let mut last_date = String::new();
    for i in 0..40u32 {
        let trend = (20.0 - i as f64 * 0.3).max(8.0);
        let date = format!("2026-01-{:02}T10:00:00+0100", i + 1);
        let guide = PriceGuide::from_entries(vec![make_test_price_entry(1, Some(trend))], &date);
        insert_price_history(&mut conn, &guide, &catalog).unwrap();
        last_date = format!("2026-01-{:02}", i + 1);
    }

    let count = run_scan(&mut conn, DEFAULT_MIN_PRICE, &last_date).unwrap();
    assert!(count >= 1, "declining card should produce a signal");

    let scan = get_buy_signals(&conn, 100).unwrap();
    assert_eq!(scan.price_date.as_deref(), Some(last_date.as_str()));
    assert_eq!(scan.signals[0]["id_product"], 1);
    assert!(scan.signals[0]["score"].as_f64().unwrap() > 0.0);
}
