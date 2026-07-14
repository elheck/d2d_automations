//! Mispricing / margin report — pure logic comparing each in-stock card's
//! listed price against a market reference price.
//!
//! This module is strictly **read-only**: it classifies listings and sums the
//! money implications so the user can decide whether to reprice. It never writes
//! prices anywhere. Resolving the market price for a card (which price-guide
//! field, foil vs non-foil) is the caller's job, supplied as a closure — this
//! keeps the module free of any Cardmarket / UI types and fully testable.

use crate::inventory_db::InStockCard;

/// Verdict for a single listing relative to the market reference price.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceVerdict {
    /// Listed meaningfully below market — money left on the table.
    Underpriced,
    /// Listed meaningfully above market — likely why it isn't selling.
    Overpriced,
    /// Within the threshold band of market.
    Fair,
    /// No usable market price for this card.
    NoMarketData,
}

impl PriceVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            PriceVerdict::Underpriced => "Underpriced",
            PriceVerdict::Overpriced => "Overpriced",
            PriceVerdict::Fair => "Fair",
            PriceVerdict::NoMarketData => "No market data",
        }
    }
}

/// One row of the mispricing report.
#[derive(Debug, Clone, PartialEq)]
pub struct MispricedCard {
    pub cardmarket_id: String,
    pub name: String,
    pub set_code: String,
    pub condition: String,
    pub is_foil: bool,
    pub location: String,
    pub quantity: i64,
    pub listed_price: f64,
    pub market_price: Option<f64>,
    /// Per-unit `listed - market` (0.0 when no market data).
    pub delta_abs: f64,
    /// `(listed - market) / market * 100` (0.0 when no market data).
    pub delta_pct: f64,
    pub verdict: PriceVerdict,
}

/// Aggregated figures across all classified rows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MispricingReport {
    pub rows: Vec<MispricedCard>,
    pub underpriced_rows: usize,
    pub underpriced_copies: i64,
    /// Extra revenue if every underpriced copy were lifted to market: Σ (market − listed) × qty.
    pub underpriced_upside: f64,
    pub overpriced_rows: usize,
    pub overpriced_copies: i64,
    /// Capital that may be stuck above market: Σ (listed − market) × qty over overpriced rows.
    pub overpriced_excess: f64,
    pub fair_rows: usize,
    pub no_data_rows: usize,
    /// Σ listed × qty over rows that have market data (comparable subset).
    pub total_listed_value: f64,
    /// Σ market × qty over rows that have market data.
    pub total_market_value: f64,
}

/// Classifies a single listing.
///
/// Returns `(verdict, delta_abs, delta_pct)`. A non-positive market price is
/// treated as missing. `threshold_pct` is the half-width of the "fair" band, in
/// percent: a listing is fair when `|delta_pct| <= threshold_pct`.
pub fn classify(listed: f64, market: Option<f64>, threshold_pct: f64) -> (PriceVerdict, f64, f64) {
    match market {
        Some(m) if m > 0.0 => {
            let delta_abs = listed - m;
            let delta_pct = delta_abs / m * 100.0;
            let verdict = if delta_pct < -threshold_pct {
                PriceVerdict::Underpriced
            } else if delta_pct > threshold_pct {
                PriceVerdict::Overpriced
            } else {
                PriceVerdict::Fair
            };
            (verdict, delta_abs, delta_pct)
        }
        _ => (PriceVerdict::NoMarketData, 0.0, 0.0),
    }
}

/// Builds the full report for `cards`, resolving each card's market price via
/// `market_of`. `threshold_pct` sets the fair band (e.g. `15.0` for ±15%).
pub fn build_report<F>(cards: &[InStockCard], threshold_pct: f64, market_of: F) -> MispricingReport
where
    F: Fn(&InStockCard) -> Option<f64>,
{
    let mut report = MispricingReport::default();

    for card in cards {
        let market = market_of(card);
        let (verdict, delta_abs, delta_pct) = classify(card.price, market, threshold_pct);
        let qty = card.quantity.max(0);

        match verdict {
            PriceVerdict::Underpriced => {
                report.underpriced_rows += 1;
                report.underpriced_copies += qty;
                // market is Some here by construction of classify.
                if let Some(m) = market {
                    report.underpriced_upside += (m - card.price) * qty as f64;
                }
            }
            PriceVerdict::Overpriced => {
                report.overpriced_rows += 1;
                report.overpriced_copies += qty;
                report.overpriced_excess += delta_abs * qty as f64;
            }
            PriceVerdict::Fair => report.fair_rows += 1,
            PriceVerdict::NoMarketData => report.no_data_rows += 1,
        }

        if let Some(m) = market {
            report.total_listed_value += card.price * qty as f64;
            report.total_market_value += m * qty as f64;
        }

        report.rows.push(MispricedCard {
            cardmarket_id: card.cardmarket_id.clone(),
            name: card.name.clone(),
            set_code: card.set_code.clone(),
            condition: card.condition.clone(),
            is_foil: card.is_foil,
            location: card.location.clone(),
            quantity: qty,
            listed_price: card.price,
            market_price: market,
            delta_abs,
            delta_pct,
            verdict,
        });
    }

    report
}

#[path = "mispricing_tests.rs"]
#[cfg(test)]
mod tests;
