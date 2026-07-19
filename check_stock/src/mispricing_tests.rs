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

fn today() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 7, 18).unwrap()
}

/// MarketData with only a reference price — the minimal market signal.
fn market(reference: f64) -> MarketData {
    MarketData {
        reference: Some(reference),
        ..MarketData::default()
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

// ==================== effective_threshold_pct ====================

#[test]
fn effective_threshold_widens_to_volatility() {
    assert_eq!(effective_threshold_pct(15.0, None), 15.0);
    // Volatility below the configured band changes nothing.
    assert_eq!(effective_threshold_pct(15.0, Some(8.0)), 15.0);
    // Volatility above it widens the band.
    assert_eq!(effective_threshold_pct(15.0, Some(25.0)), 25.0);
}

// ==================== momentum_of ====================

#[test]
fn momentum_short_window_wins_when_conclusive() {
    // avg1 +10% vs avg7 → rising, even though avg7 vs avg30 is falling.
    let (m, short, long) = momentum_of(Some(1.10), Some(1.0), Some(1.20));
    assert_eq!(m, Momentum::Rising);
    assert!((short.unwrap() - 10.0).abs() < 0.001);
    assert!(long.unwrap() < -MOMENTUM_THRESHOLD_PCT);
}

#[test]
fn momentum_falls_back_to_long_window() {
    // Short window flat (+1%), long window −20% → falling.
    let (m, _, _) = momentum_of(Some(1.01), Some(1.0), Some(1.25));
    assert_eq!(m, Momentum::Falling);
    // Short window missing entirely → long window decides.
    let (m, short, _) = momentum_of(None, Some(1.0), Some(0.8));
    assert_eq!(m, Momentum::Rising);
    assert_eq!(short, None);
}

#[test]
fn momentum_flat_and_unknown() {
    let (m, _, _) = momentum_of(Some(1.01), Some(1.0), Some(1.0));
    assert_eq!(m, Momentum::Flat);
    // Only the short window present, and it is flat.
    let (m, _, long) = momentum_of(Some(1.01), Some(1.0), None);
    assert_eq!(m, Momentum::Flat);
    assert_eq!(long, None);
    let (m, _, _) = momentum_of(None, None, None);
    assert_eq!(m, Momentum::Unknown);
}

// ==================== action_for ====================

#[test]
fn action_matrix() {
    use Momentum::*;
    use PriceVerdict::*;
    assert_eq!(action_for(Underpriced, Rising), Action::RaiseNow);
    assert_eq!(action_for(Underpriced, Falling), Action::Watch);
    assert_eq!(action_for(Underpriced, Flat), Action::Raise);
    assert_eq!(action_for(Underpriced, Unknown), Action::Raise);
    assert_eq!(action_for(Overpriced, Falling), Action::CutNow);
    assert_eq!(action_for(Overpriced, Rising), Action::Hold);
    assert_eq!(action_for(Overpriced, Flat), Action::Cut);
    assert_eq!(action_for(Overpriced, Unknown), Action::Cut);
    assert_eq!(action_for(Fair, Rising), Action::None);
    assert_eq!(action_for(NoMarketData, Falling), Action::None);
}

// ==================== priority_score ====================

#[test]
fn priority_urgent_actions_outrank_plain_ones() {
    let urgent = priority_score(1.0, 2, Action::RaiseNow, 0, false);
    let plain = priority_score(1.0, 2, Action::Raise, 0, false);
    let watch = priority_score(1.0, 2, Action::Watch, 0, false);
    assert!(urgent > plain && plain > watch);
    assert_eq!(priority_score(1.0, 2, Action::None, 0, false), 0.0);
}

#[test]
fn priority_age_boosts_overpriced_only() {
    // A year-old overpriced listing doubles its score …
    let old = priority_score(1.0, 1, Action::Cut, 365, false);
    let new = priority_score(1.0, 1, Action::Cut, 0, false);
    assert!((old - 2.0 * new).abs() < 0.001);
    // … while underpriced rows are unaffected by age.
    let old_up = priority_score(1.0, 1, Action::Raise, 365, false);
    let new_up = priority_score(1.0, 1, Action::Raise, 0, false);
    assert!((old_up - new_up).abs() < 0.001);
}

#[test]
fn priority_stale_data_halves_score() {
    let fresh = priority_score(2.0, 1, Action::Cut, 0, false);
    let stale = priority_score(2.0, 1, Action::Cut, 0, true);
    assert!((stale - fresh / 2.0).abs() < 0.001);
}

#[test]
fn priority_negative_quantity_clamped() {
    assert_eq!(priority_score(1.0, -3, Action::Cut, 0, false), 0.0);
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
        "1" | "2" | "3" => market(2.0),
        _ => MarketData::default(),
    };
    let r = build_report(&cards, 15.0, today(), markets);

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
    // No momentum/volatility/date data → no urgent or stale rows.
    assert_eq!(r.raise_now_rows, 0);
    assert_eq!(r.cut_now_rows, 0);
    assert_eq!(r.stale_rows, 0);
}

#[test]
fn report_foil_market_resolution_is_callers_choice() {
    // The closure decides foil vs non-foil pricing; build_report just applies it.
    let cards = vec![card("1", 10.0, 1, true)];
    let r = build_report(&cards, 10.0, today(), |c| {
        if c.is_foil {
            market(20.0)
        } else {
            market(5.0)
        }
    });
    assert_eq!(r.rows[0].verdict, PriceVerdict::Underpriced);
    assert_eq!(r.rows[0].market_price, Some(20.0));
}

#[test]
fn report_empty_is_all_zero() {
    let r = build_report(&[], 15.0, today(), |_| market(1.0));
    assert_eq!(r, MispricingReport::default());
}

#[test]
fn report_below_low_escalates_fair_to_underpriced() {
    // Listed −10% vs reference (in the ±15% band), but below the market's
    // cheapest listing → escalated to underpriced.
    let cards = vec![card("1", 9.0, 1, false)];
    let r = build_report(&cards, 15.0, today(), |_| MarketData {
        reference: Some(10.0),
        low: Some(9.5),
        ..MarketData::default()
    });
    let row = &r.rows[0];
    assert!(row.below_low);
    assert_eq!(row.verdict, PriceVerdict::Underpriced);
    assert_eq!(r.underpriced_rows, 1);
    assert!((r.underpriced_upside - 1.0).abs() < 0.001);
}

#[test]
fn report_below_low_needs_market_data() {
    // No reference price → below-low cannot be judged, verdict stays NoMarketData.
    let cards = vec![card("1", 1.0, 1, false)];
    let r = build_report(&cards, 15.0, today(), |_| MarketData {
        low: Some(2.0),
        ..MarketData::default()
    });
    assert!(!r.rows[0].below_low);
    assert_eq!(r.rows[0].verdict, PriceVerdict::NoMarketData);
}

#[test]
fn report_volatility_widens_fair_band() {
    // −20% delta would be underpriced at ±15%, but the card swings ±30%.
    let cards = vec![card("1", 8.0, 1, false)];
    let r = build_report(&cards, 15.0, today(), |_| MarketData {
        reference: Some(10.0),
        volatility_pct: Some(30.0),
        ..MarketData::default()
    });
    let row = &r.rows[0];
    assert_eq!(row.verdict, PriceVerdict::Fair);
    assert_eq!(row.effective_threshold_pct, 30.0);
    assert_eq!(row.action, Action::None);
    assert_eq!(row.priority, 0.0);
}

#[test]
fn report_momentum_drives_action_and_urgent_counts() {
    let cards = vec![
        card("1", 1.0, 1, false), // underpriced + rising → RaiseNow
        card("2", 5.0, 1, false), // overpriced + falling → CutNow
    ];
    let r = build_report(&cards, 15.0, today(), |c| {
        let (avg1, avg7, avg30) = match c.cardmarket_id.as_str() {
            "1" => (Some(2.4), Some(2.0), Some(2.0)), // +20% short window
            _ => (Some(1.6), Some(2.0), Some(2.0)),   // −20% short window
        };
        MarketData {
            reference: Some(2.0),
            avg1,
            avg7,
            avg30,
            ..MarketData::default()
        }
    });
    assert_eq!(r.rows[0].momentum, Momentum::Rising);
    assert_eq!(r.rows[0].action, Action::RaiseNow);
    assert_eq!(r.rows[1].momentum, Momentum::Falling);
    assert_eq!(r.rows[1].action, Action::CutNow);
    assert_eq!(r.raise_now_rows, 1);
    assert_eq!(r.cut_now_rows, 1);
}

#[test]
fn report_stale_market_data_is_flagged_and_counted() {
    let cards = vec![card("1", 1.0, 1, false), card("2", 1.0, 1, false)];
    let r = build_report(&cards, 15.0, today(), |c| MarketData {
        reference: Some(2.0),
        price_date: match c.cardmarket_id.as_str() {
            // 10 days old → stale; the other row is fresh.
            "1" => chrono::NaiveDate::from_ymd_opt(2026, 7, 8),
            _ => chrono::NaiveDate::from_ymd_opt(2026, 7, 17),
        },
        ..MarketData::default()
    });
    assert!(r.rows[0].stale);
    assert!(!r.rows[1].stale);
    assert_eq!(r.stale_rows, 1);
    // Same mispricing, but the stale row scores half the priority.
    assert!((r.rows[0].priority - r.rows[1].priority / 2.0).abs() < 0.001);
}

#[test]
fn report_age_days_and_recently_listed() {
    // Fixture cards are listed 2026-01-01; today is 2026-07-18 → 198 days.
    let mut fresh = card("2", 1.0, 1, false);
    fresh.effective_date = "2026-07-18".to_string();
    let cards = vec![card("1", 1.0, 1, false), fresh];
    let r = build_report(&cards, 15.0, today(), |_| market(2.0));
    assert_eq!(r.rows[0].age_days, 198);
    assert!(!r.rows[0].recently_listed());
    assert_eq!(r.rows[1].age_days, 0);
    assert!(r.rows[1].recently_listed());
}
