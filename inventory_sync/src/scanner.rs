//! Buy-signal scanner: ranks products by how good an undervalued dip-buy they are.
//!
//! Strategy: **undervalued dip-buys**. We look for cards trading below their own
//! recent trend and history — oversold, sitting near their price floor, low in
//! their historical range — ideally just as short-term momentum turns back up.
//! The goal is to buy cheap now and sell back toward trend later.
//!
//! The scoring here is pure (no DB, no I/O) so it is fully unit-testable. It
//! consumes the per-product price series produced by
//! [`crate::database::get_recent_series_for_scan`] and returns a ranked list of
//! [`BuySignal`]s.

use rusqlite::Connection;
use serde::Serialize;

use crate::database::{
    get_recent_series_for_scan, replace_buy_signals, BuySignalRow, DbResult, PriceHistoryPoint,
    ProductPriceSeries,
};
use crate::indicators::{
    calculate_bb_percent_b, calculate_bollinger_bands, calculate_roc, calculate_rsi,
};

/// Minimum number of price points a product needs before we score it.
/// Below this the indicators (RSI/Bollinger) don't have enough data to be meaningful.
pub const MIN_POINTS: usize = 15;

/// A single scored buy candidate, ready to serialize to the web client.
#[derive(Debug, Clone, Serialize)]
pub struct BuySignal {
    pub id_product: u64,
    pub name: String,
    pub category_name: String,
    pub id_expansion: u64,
    pub expansion_name: Option<String>,
    /// Overall buy score, 0–100. Higher = stronger undervalued dip-buy.
    pub score: f64,
    /// Most recent trend price (what the card is "worth" right now).
    pub trend: Option<f64>,
    /// Most recent low price (cheapest current listing — the buy target).
    pub low: Option<f64>,
    /// low ÷ trend for the latest day (< 0.8 = heavy undercutting near the floor).
    pub floor_ratio: Option<f64>,
    /// Latest RSI (0–100). Below 30 = oversold.
    pub rsi: Option<f64>,
    /// Latest Bollinger %B (0 = at lower band / cheap, 1 = at upper band / expensive).
    pub bb_percent_b: Option<f64>,
    /// 30-day rate of change in %. Negative = the card has dropped recently.
    pub roc_30: Option<f64>,
    /// avg1 − avg7: positive = short-term price starting to turn back up (a bounce).
    pub momentum_1_7: Option<f64>,
    /// Human-readable reasons that contributed to the score, strongest first.
    pub reasons: Vec<String>,
}

/// The individual signal contributions for one product, before weighting.
///
/// Each field is a 0.0–1.0 sub-score where 1.0 is the strongest "buy" reading.
struct SubScores {
    /// Price near its floor (low well below trend).
    floor: f64,
    /// Oversold RSI.
    rsi: f64,
    /// Low in its own historical Bollinger range.
    position: f64,
    /// Has dropped recently (negative 30d ROC) — the "dip".
    dip: f64,
    /// Short-term momentum turning up after the dip (avg1 rising above avg7).
    bounce: f64,
}

/// Weights for combining sub-scores into the final 0–100 score.
/// They sum to 1.0 so the composite stays in [0, 1] before scaling.
const W_FLOOR: f64 = 0.28;
const W_RSI: f64 = 0.24;
const W_POSITION: f64 = 0.22;
const W_DIP: f64 = 0.14;
const W_BOUNCE: f64 = 0.12;

/// Scan and rank all supplied product series as undervalued dip-buys.
///
/// Returns candidates sorted by descending score. Products with fewer than
/// [`MIN_POINTS`] price points, or that produce a zero score, are dropped.
/// At most `limit` results are returned.
pub fn scan_buy_signals(series: &[ProductPriceSeries], limit: usize) -> Vec<BuySignal> {
    let mut signals: Vec<BuySignal> = series
        .iter()
        .filter_map(score_series)
        .filter(|s| s.score > 0.0)
        .collect();

    // Sort by score descending; ties broken by cheaper floor_ratio (more undercut first).
    signals.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.floor_ratio
                    .unwrap_or(f64::INFINITY)
                    .partial_cmp(&b.floor_ratio.unwrap_or(f64::INFINITY))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    signals.truncate(limit);
    signals
}

/// How many days of history to pull into the scan window.
/// 120 days is enough for the 20-day Bollinger and 30-day ROC to warm up with margin.
pub const SCAN_WINDOW_DAYS: u64 = 120;

/// Default minimum latest-trend price (in EUR) for a card to be considered.
/// Sub-€1 penny cards are noisy and not worth buying, so they're filtered out.
pub const DEFAULT_MIN_PRICE: f64 = 1.0;

/// Maximum number of ranked signals to compute and store.
pub const MAX_STORED_SIGNALS: usize = 500;

/// Run a full buy-signal scan against the database and persist the ranked results.
///
/// Pulls the recent price window for every product trading at or above
/// `min_price`, scores them as undervalued dip-buys, and replaces the stored
/// `buy_signals` table transactionally. Intended to run once per day right after
/// new price data is ingested. `price_date` records which day's data was scanned.
///
/// Returns the number of signals stored.
pub fn run_scan(conn: &mut Connection, min_price: f64, price_date: &str) -> DbResult<usize> {
    let since = scan_since_date(price_date);
    let series = get_recent_series_for_scan(conn, &since, min_price)?;
    let signals = scan_buy_signals(&series, MAX_STORED_SIGNALS);

    let rows: Vec<BuySignalRow> = signals
        .iter()
        .map(|s| BuySignalRow {
            id_product: s.id_product,
            score: s.score,
            // Serialization of our own struct cannot fail in practice.
            payload: serde_json::to_string(s).unwrap_or_else(|_| "null".to_string()),
        })
        .collect();

    let count = rows.len();
    replace_buy_signals(conn, &rows, price_date)?;
    Ok(count)
}

/// Compute the window start date (`price_date` minus [`SCAN_WINDOW_DAYS`]).
///
/// Falls back to an all-history scan (`"0000-01-01"`) if the date can't be parsed.
fn scan_since_date(price_date: &str) -> String {
    chrono::NaiveDate::parse_from_str(price_date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.checked_sub_days(chrono::Days::new(SCAN_WINDOW_DAYS)))
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "0000-01-01".to_string())
}

/// Score a single product's price series. Returns `None` if it lacks enough data.
fn score_series(s: &ProductPriceSeries) -> Option<BuySignal> {
    if s.points.len() < MIN_POINTS {
        return None;
    }

    // Trend series drives the technical indicators. Skip products with gaps in trend.
    let trend_series: Vec<f64> = s.points.iter().filter_map(|p| p.trend).collect();
    if trend_series.len() < MIN_POINTS {
        return None;
    }

    let latest = s.points.last()?;

    let rsi = calculate_rsi(&trend_series, 14);
    let roc_30 = calculate_roc(&trend_series, 30);
    let (bb_upper, _bb_mid, bb_lower) = calculate_bollinger_bands(&trend_series, 20, 2.0);
    let bb_pb = calculate_bb_percent_b(&trend_series, &bb_upper, &bb_lower);

    let latest_rsi = last_some(&rsi);
    let latest_roc_30 = last_some(&roc_30);
    let latest_bb_pb = last_some(&bb_pb);
    let floor_ratio = floor_ratio(latest);
    let momentum_1_7 = momentum_1_7(latest);

    let sub = compute_sub_scores(
        floor_ratio,
        latest_rsi,
        latest_bb_pb,
        latest_roc_30,
        momentum_1_7,
    );

    let composite = sub.floor * W_FLOOR
        + sub.rsi * W_RSI
        + sub.position * W_POSITION
        + sub.dip * W_DIP
        + sub.bounce * W_BOUNCE;

    // Corroboration gate: a floor undercut on its own is noisy (often one mispriced
    // listing). Require a technical signal — oversold RSI, low band position, or a
    // recent dip — to confirm it. With no corroboration, halve the score so these
    // don't dominate the ranking; a strong corroborated case is unaffected.
    let corroborated = sub.rsi > 0.0 || sub.position > 0.0 || sub.dip > 0.0;
    let composite = if corroborated {
        composite
    } else {
        composite * 0.5
    };

    let score = (composite * 100.0).clamp(0.0, 100.0);

    let reasons = build_reasons(&sub, floor_ratio, latest_rsi, latest_bb_pb, latest_roc_30);

    Some(BuySignal {
        id_product: s.id_product,
        name: s.name.clone(),
        category_name: s.category_name.clone(),
        id_expansion: s.id_expansion,
        expansion_name: s.expansion_name.clone(),
        score: round2(score),
        trend: latest.trend,
        low: latest.low,
        floor_ratio: floor_ratio.map(round2),
        rsi: latest_rsi.map(round2),
        bb_percent_b: latest_bb_pb.map(round2),
        roc_30: latest_roc_30.map(round2),
        momentum_1_7: momentum_1_7.map(round2),
        reasons,
    })
}

/// low ÷ trend for a single day. `None` if either is missing or trend is ~0.
fn floor_ratio(p: &PriceHistoryPoint) -> Option<f64> {
    match (p.low, p.trend) {
        (Some(l), Some(t)) if t.abs() > 1e-10 => Some(l / t),
        _ => None,
    }
}

/// avg1 − avg7 for a single day. `None` if either is missing.
fn momentum_1_7(p: &PriceHistoryPoint) -> Option<f64> {
    match (p.avg1, p.avg7) {
        (Some(a1), Some(a7)) => Some(a1 - a7),
        _ => None,
    }
}

/// Compute each 0.0–1.0 sub-score from the latest indicator readings.
///
/// A missing indicator contributes 0.0 (neutral, not a buy reason).
fn compute_sub_scores(
    floor_ratio: Option<f64>,
    rsi: Option<f64>,
    bb_pb: Option<f64>,
    roc_30: Option<f64>,
    momentum_1_7: Option<f64>,
) -> SubScores {
    // Floor signal, with a guard against noise. A `low` far below trend is almost
    // always a single mispriced or damaged listing, not a real buy opportunity, so
    // the signal peaks in a *sane* undercut band and is discounted below it:
    //   ratio >= 1.0 (at/above trend)      → 0.0  (not undervalued)
    //   ratio 0.75 (healthy 25% undercut)  → 1.0  (best genuine dip)
    //   ratio 0.50                          → ~0.5
    //   ratio <= 0.30 (implausible)         → 0.0  (treated as bad data)
    let floor = floor_ratio
        .map(|r| {
            if !(0.30..1.0).contains(&r) {
                0.0
            } else if r >= 0.75 {
                // Linear ramp up from at-trend (0) to a 25% undercut (1.0).
                ((1.0 - r) / 0.25).clamp(0.0, 1.0)
            } else {
                // Below a 25% undercut, taper back toward 0 as the price gets
                // implausibly cheap (0.75 → 1.0, 0.30 → 0.0).
                ((r - 0.30) / 0.45).clamp(0.0, 1.0)
            }
        })
        .unwrap_or(0.0);

    // RSI: 50 or above → 0.0; 20 or below → 1.0. Linear between.
    let rsi = rsi
        .map(|v| ((50.0 - v) / 30.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    // Bollinger %B: 0.5 (mid-band) → 0.0; 0.0 or below (at/under lower band) → 1.0.
    let position = bb_pb
        .map(|v| ((0.5 - v) / 0.5).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    // Dip: 0% 30d change → 0.0; −25% or worse → 1.0. Positive ROC is not a dip.
    let dip = roc_30.map(|v| ((-v) / 25.0).clamp(0.0, 1.0)).unwrap_or(0.0);

    // Bounce: positive momentum (avg1 > avg7) → up to 1.0 at +5% of… we scale on
    // the raw currency delta being positive. We only reward a *turn up*, so
    // negative momentum contributes 0. Saturates quickly since these are small deltas.
    let bounce = momentum_1_7
        .map(|m| (m / 0.05_f64.max(1e-9)).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    SubScores {
        floor,
        rsi,
        position,
        dip,
        bounce,
    }
}

/// Build human-readable reason strings, strongest sub-score first.
fn build_reasons(
    sub: &SubScores,
    floor_ratio: Option<f64>,
    rsi: Option<f64>,
    bb_pb: Option<f64>,
    roc_30: Option<f64>,
) -> Vec<String> {
    let mut scored: Vec<(f64, String)> = Vec::new();

    if sub.floor > 0.0 {
        if let Some(r) = floor_ratio {
            scored.push((
                sub.floor * W_FLOOR,
                format!(
                    "Listings {:.0}% below trend (floor ratio {:.2})",
                    (1.0 - r) * 100.0,
                    r
                ),
            ));
        }
    }
    if sub.rsi > 0.0 {
        if let Some(v) = rsi {
            scored.push((sub.rsi * W_RSI, format!("Oversold (RSI {:.0})", v)));
        }
    }
    if sub.position > 0.0 {
        if let Some(v) = bb_pb {
            scored.push((
                sub.position * W_POSITION,
                format!("Low in its historical range ({:.0}% of band)", v * 100.0),
            ));
        }
    }
    if sub.dip > 0.0 {
        if let Some(v) = roc_30 {
            scored.push((sub.dip * W_DIP, format!("Down {:.0}% over 30 days", -v)));
        }
    }
    if sub.bounce > 0.0 {
        scored.push((
            sub.bounce * W_BOUNCE,
            "Short-term price turning back up".to_string(),
        ));
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().map(|(_, r)| r).collect()
}

/// Last non-`None` value in an indicator series.
fn last_some(series: &[Option<f64>]) -> Option<f64> {
    series.iter().rev().find_map(|v| *v)
}

/// Round to 2 decimal places for stable JSON output.
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
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
        use crate::cardmarket::{
            make_test_price_entry, make_test_product, PriceGuide, ProductCatalog,
        };
        use crate::database::{
            get_buy_signals, init_schema, insert_price_history, upsert_products,
        };

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
            let guide =
                PriceGuide::from_entries(vec![make_test_price_entry(1, Some(trend))], &date);
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
}
