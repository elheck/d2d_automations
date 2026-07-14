use super::*;
use crate::inventory_db::InStockCard;

fn card(id: &str, price: f64, quantity: i64, is_foil: bool) -> InStockCard {
    InStockCard {
        cardmarket_id: id.to_string(),
        name: format!("Card {id}"),
        set_code: "TST".to_string(),
        cn: "1".to_string(),
        condition: "NM".to_string(),
        language: "English".to_string(),
        is_foil,
        rarity: "rare".to_string(),
        quantity,
        price,
        location: "A-0-1-1".to_string(),
        effective_date: "2026-01-01".to_string(),
    }
}

// ==================== classify ====================

#[test]
fn classify_underpriced() {
    let (v, abs, pct) = classify(1.0, Some(2.0), 15.0);
    assert_eq!(v, PriceVerdict::Underpriced);
    assert!((abs - -1.0).abs() < 0.001);
    assert!((pct - -50.0).abs() < 0.001);
}

#[test]
fn classify_overpriced() {
    let (v, _, pct) = classify(3.0, Some(2.0), 15.0);
    assert_eq!(v, PriceVerdict::Overpriced);
    assert!((pct - 50.0).abs() < 0.001);
}

#[test]
fn classify_fair_within_band() {
    let (v, _, _) = classify(2.1, Some(2.0), 15.0); // +5%
    assert_eq!(v, PriceVerdict::Fair);
}

#[test]
fn classify_exactly_on_threshold_is_fair() {
    // +15% with a 15% band is inclusive → fair.
    let (v, _, _) = classify(2.3, Some(2.0), 15.0);
    assert_eq!(v, PriceVerdict::Fair);
}

#[test]
fn classify_missing_or_zero_market_is_no_data() {
    assert_eq!(classify(1.0, None, 15.0).0, PriceVerdict::NoMarketData);
    assert_eq!(classify(1.0, Some(0.0), 15.0).0, PriceVerdict::NoMarketData);
    assert_eq!(
        classify(1.0, Some(-5.0), 15.0).0,
        PriceVerdict::NoMarketData
    );
}

// ==================== build_report ====================

#[test]
fn report_totals_and_upside() {
    let cards = vec![
        card("1", 1.0, 2, false), // market 2.0 → underpriced, upside (2-1)*2 = 2
        card("2", 5.0, 1, false), // market 2.0 → overpriced, excess (5-2)*1 = 3
        card("3", 2.0, 3, false), // market 2.0 → fair
        card("4", 9.0, 1, false), // no market data
    ];
    let markets = |c: &InStockCard| match c.cardmarket_id.as_str() {
        "1" | "2" | "3" => Some(2.0),
        _ => None,
    };
    let r = build_report(&cards, 15.0, markets);

    assert_eq!(r.rows.len(), 4);
    assert_eq!(r.underpriced_rows, 1);
    assert_eq!(r.underpriced_copies, 2);
    assert!((r.underpriced_upside - 2.0).abs() < 0.001);
    assert_eq!(r.overpriced_rows, 1);
    assert_eq!(r.overpriced_copies, 1);
    assert!((r.overpriced_excess - 3.0).abs() < 0.001);
    assert_eq!(r.fair_rows, 1);
    assert_eq!(r.no_data_rows, 1);
    // Comparable subset excludes card 4.
    assert!((r.total_listed_value - (1.0 * 2.0 + 5.0 + 2.0 * 3.0)).abs() < 0.001);
    assert!((r.total_market_value - (2.0 * 2.0 + 2.0 + 2.0 * 3.0)).abs() < 0.001);
}

#[test]
fn report_foil_market_resolution_is_callers_choice() {
    // The closure decides foil vs non-foil pricing; build_report just applies it.
    let cards = vec![card("1", 10.0, 1, true)];
    let r = build_report(
        &cards,
        10.0,
        |c| if c.is_foil { Some(20.0) } else { Some(5.0) },
    );
    assert_eq!(r.rows[0].verdict, PriceVerdict::Underpriced);
    assert_eq!(r.rows[0].market_price, Some(20.0));
}

#[test]
fn report_empty_is_all_zero() {
    let r = build_report(&[], 15.0, |_| Some(1.0));
    assert_eq!(r, MispricingReport::default());
}
