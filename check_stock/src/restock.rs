//! Restock recommendations — pure ranking of sold-out variants by sales speed.
//!
//! Answers "what should I buy again?": takes the sold-out variants pulled from
//! the inventory DB ([`RestockCandidate`]) and ranks them by how quickly they
//! sold through, filtering out one-off sales. This module is pure and free of
//! any database or wall-clock access so it can be tested deterministically —
//! the sell-through window is measured entirely from the dates carried by the
//! candidates (listing date → last sale date).

use crate::inventory_db::RestockCandidate;
use chrono::NaiveDate;

/// A restock candidate with its derived sell-through metrics, ready to display.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedRestock {
    pub candidate: RestockCandidate,
    /// Days from listing to the last recorded sale, clamped to ≥ 1 so velocity
    /// is always finite (a same-day sell-out counts as one day).
    pub days_to_sell_out: i64,
    /// Average copies sold per week over the sell-through window.
    pub copies_per_week: f64,
}

/// Parses the leading `YYYY-MM-DD` of a date string (tolerates trailing time).
fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10).unwrap_or(s), "%Y-%m-%d").ok()
}

/// Days from listing to sell-out, clamped to ≥ 1. Unparseable or reversed dates
/// collapse to 1 day — such rows still appear rather than being silently dropped.
fn sell_out_days(c: &RestockCandidate) -> i64 {
    match (parse_date(&c.listed_date), parse_date(&c.sold_out_date)) {
        (Some(listed), Some(sold_out)) => (sold_out - listed).num_days().max(1),
        _ => 1,
    }
}

/// Filters and ranks restock candidates.
///
/// Variants that sold fewer than `min_copies` total are dropped (a single sale
/// is no evidence of demand). The rest are ordered fastest-selling first
/// (copies/week), with realized revenue as the tie-breaker and name as a stable
/// final tie-breaker.
pub fn rank_candidates(candidates: Vec<RestockCandidate>, min_copies: i64) -> Vec<RankedRestock> {
    let mut ranked: Vec<RankedRestock> = candidates
        .into_iter()
        .filter(|c| c.sold_copies >= min_copies)
        .map(|c| {
            let days = sell_out_days(&c);
            RankedRestock {
                days_to_sell_out: days,
                copies_per_week: c.sold_copies as f64 * 7.0 / days as f64,
                candidate: c,
            }
        })
        .collect();

    ranked.sort_by(|a, b| {
        b.copies_per_week
            .partial_cmp(&a.copies_per_week)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.candidate
                    .realized_revenue
                    .partial_cmp(&a.candidate.realized_revenue)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.candidate.name.cmp(&b.candidate.name))
    });
    ranked
}

/// Formats ranked candidates as a buy-list CSV (comma-separated, headers first).
///
/// Column names follow the camelCase convention of the other CSV exports. The
/// list is written in the order given, so callers export exactly what they see.
pub fn format_buy_list_csv(rows: &[RankedRestock]) -> String {
    use csv::WriterBuilder;

    let mut wtr = WriterBuilder::new().has_headers(true).from_writer(vec![]);
    let _ = wtr.write_record([
        "name",
        "setCode",
        "cn",
        "condition",
        "language",
        "isFoil",
        "rarity",
        "soldCopies",
        "copiesPerWeek",
        "daysToSellOut",
        "lastPrice",
        "realizedRevenue",
    ]);

    for r in rows {
        let c = &r.candidate;
        let _ = wtr.write_record([
            c.name.as_str(),
            c.set_code.as_str(),
            c.cn.as_str(),
            c.condition.as_str(),
            c.language.as_str(),
            if c.is_foil { "1" } else { "" },
            c.rarity.as_str(),
            &c.sold_copies.to_string(),
            &format!("{:.2}", r.copies_per_week),
            &r.days_to_sell_out.to_string(),
            &format!("{:.2}", c.last_price),
            &format!("{:.2}", c.realized_revenue),
        ]);
    }

    String::from_utf8(wtr.into_inner().unwrap_or_default()).unwrap_or_default()
}

#[path = "restock_tests.rs"]
#[cfg(test)]
mod tests;
