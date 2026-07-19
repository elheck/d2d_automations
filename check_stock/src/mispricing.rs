//! Mispricing / margin report — pure logic comparing each in-stock card's
//! listed price against market data.
//!
//! This module is strictly **read-only**: it classifies listings and sums the
//! money implications so the user can decide whether to reprice. It never writes
//! prices anywhere. Resolving the market data for a card (which price-guide
//! field, foil vs non-foil, volatility from snapshots) is the caller's job,
//! supplied as a closure — this keeps the module free of any Cardmarket / UI
//! types and fully testable.
//!
//! Beyond the basic listed-vs-reference band check, each row is enriched with:
//! - **Band position** — listed below the market's cheapest listing (`low`)
//!   escalates an in-band verdict to underpriced (you are guaranteed cheapest).
//! - **Momentum** — Cardmarket's avg1/avg7/avg30 rolling averages crossed with
//!   the verdict yield an [`Action`] ("raise now" beats "raise" when the
//!   market is spiking underneath a cheap listing).
//! - **Volatility-widened fair band** — a delta inside a card's own ~90-day
//!   swing is noise, not a mispricing.
//! - **Staleness** — market rows older than [`STALE_AFTER_DAYS`] lower
//!   confidence instead of being treated as current.
//! - **Listing age** — old overpriced stock is dead capital and gains urgency.
//! - **Priority score** — one transparent number to sort "what to fix first".

use crate::aging::age_days;
use crate::inventory_db::InStockCard;
use crate::price_trends::pct_change;
use chrono::NaiveDate;

/// Momentum windows smaller than this (in %) count as flat market.
pub const MOMENTUM_THRESHOLD_PCT: f64 = 3.0;

/// Market rows older than this many days are flagged stale (low confidence).
pub const STALE_AFTER_DAYS: i64 = 7;

/// Listings at most this old count as "new" — an underpriced brand-new listing
/// is likely a typo rather than market movement.
pub const NEW_LISTING_MAX_AGE_DAYS: i64 = 1;

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

/// Direction of the market price, judged from Cardmarket's rolling averages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Momentum {
    Rising,
    Falling,
    Flat,
    /// Not enough average data to judge.
    #[default]
    Unknown,
}

impl Momentum {
    pub fn as_str(self) -> &'static str {
        match self {
            Momentum::Rising => "Rising",
            Momentum::Falling => "Falling",
            Momentum::Flat => "Flat",
            Momentum::Unknown => "Unknown",
        }
    }
}

/// What to do about a listing: the verdict crossed with market momentum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Action {
    /// Underpriced while the market rises — selling cheap into a spike.
    RaiseNow,
    /// Underpriced in a flat (or unknown-direction) market.
    Raise,
    /// Underpriced but the market is falling toward the listing — low urgency.
    Watch,
    /// Overpriced while the market falls — cut before it falls further.
    CutNow,
    /// Overpriced in a flat (or unknown-direction) market.
    Cut,
    /// Overpriced but the market is rising toward the listing.
    Hold,
    /// Fairly priced or no data — nothing to do.
    #[default]
    None,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::RaiseNow => "Raise now",
            Action::Raise => "Raise",
            Action::Watch => "Watch",
            Action::CutNow => "Cut now",
            Action::Cut => "Cut",
            Action::Hold => "Hold",
            Action::None => "—",
        }
    }

    /// Urgency multiplier used by [`priority_score`].
    fn weight(self) -> f64 {
        match self {
            Action::RaiseNow | Action::CutNow => 2.0,
            Action::Raise | Action::Cut => 1.0,
            Action::Watch | Action::Hold => 0.4,
            Action::None => 0.0,
        }
    }
}

/// Everything the market source knows about one card, resolved foil-aware by
/// the caller. All fields are optional — missing data degrades the analysis
/// instead of failing it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MarketData {
    /// The chosen reference price the listing is compared against.
    pub reference: Option<f64>,
    /// Cheapest listing on the market (any condition).
    pub low: Option<f64>,
    /// Cardmarket 1-day rolling average sale price.
    pub avg1: Option<f64>,
    /// Cardmarket 7-day rolling average sale price.
    pub avg7: Option<f64>,
    /// Cardmarket 30-day rolling average sale price.
    pub avg30: Option<f64>,
    /// Date of the market row, for staleness detection (`None` = unknown,
    /// never flagged stale).
    pub price_date: Option<NaiveDate>,
    /// Relative volatility of the reference price over recent history
    /// (coefficient of variation, in %). Widens the fair band.
    pub volatility_pct: Option<f64>,
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
    /// Cheapest listing on the market, when known.
    pub market_low: Option<f64>,
    /// Per-unit `listed - market` (0.0 when no market data).
    pub delta_abs: f64,
    /// `(listed - market) / market * 100` (0.0 when no market data).
    pub delta_pct: f64,
    pub verdict: PriceVerdict,
    /// avg1 vs avg7 change in % (short-term market momentum).
    pub momentum_short_pct: Option<f64>,
    /// avg7 vs avg30 change in % (long-term market momentum).
    pub momentum_long_pct: Option<f64>,
    pub momentum: Momentum,
    /// Listed strictly below the market's cheapest listing.
    pub below_low: bool,
    /// Market row older than [`STALE_AFTER_DAYS`].
    pub stale: bool,
    /// Relative volatility of the market reference, when known (in %).
    pub volatility_pct: Option<f64>,
    /// The fair band actually applied to this row (threshold widened to the
    /// card's own volatility).
    pub effective_threshold_pct: f64,
    /// Days since the card was listed (see [`crate::aging::age_days`]).
    pub age_days: i64,
    pub action: Action,
    /// Sort key for "what to fix first" — see [`priority_score`].
    pub priority: f64,
}

impl MispricedCard {
    /// True when the listing is new enough that an underpriced verdict is
    /// more likely a typo than market movement.
    pub fn recently_listed(&self) -> bool {
        self.age_days <= NEW_LISTING_MAX_AGE_DAYS
    }
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
    /// Rows demanding immediate action: underpriced in a rising market.
    pub raise_now_rows: usize,
    /// Rows demanding immediate action: overpriced in a falling market.
    pub cut_now_rows: usize,
    /// Rows whose market data is older than [`STALE_AFTER_DAYS`].
    pub stale_rows: usize,
    /// Σ listed × qty over rows that have market data (comparable subset).
    pub total_listed_value: f64,
    /// Σ market × qty over rows that have market data.
    pub total_market_value: f64,
}

/// Classifies a single listing against the reference price.
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

/// The fair band actually applied to a card: at least `threshold_pct`, widened
/// to the card's own volatility when that is larger. A delta inside the card's
/// natural swing is noise, not an actionable mispricing.
pub fn effective_threshold_pct(threshold_pct: f64, volatility_pct: Option<f64>) -> f64 {
    volatility_pct.map_or(threshold_pct, |v| threshold_pct.max(v))
}

/// Classifies market momentum from the avg1/avg7/avg30 rolling averages.
///
/// Returns `(momentum, short_pct, long_pct)` where `short_pct` is the avg1 vs
/// avg7 change and `long_pct` the avg7 vs avg30 change, both in percent.
///
/// The short window is checked first — it reacts fastest and is what matters
/// for repricing. When it is inconclusive (missing or within
/// ±[`MOMENTUM_THRESHOLD_PCT`]), the long window decides.
pub fn momentum_of(
    avg1: Option<f64>,
    avg7: Option<f64>,
    avg30: Option<f64>,
) -> (Momentum, Option<f64>, Option<f64>) {
    let short = pct_change(avg7, avg1);
    let long = pct_change(avg30, avg7);
    let judge = |pct: f64| {
        if pct > MOMENTUM_THRESHOLD_PCT {
            Momentum::Rising
        } else if pct < -MOMENTUM_THRESHOLD_PCT {
            Momentum::Falling
        } else {
            Momentum::Flat
        }
    };
    let momentum = match (short, long) {
        (Some(s), _) if s.abs() > MOMENTUM_THRESHOLD_PCT => judge(s),
        (_, Some(l)) => judge(l),
        (Some(_), None) => Momentum::Flat,
        (None, None) => Momentum::Unknown,
    };
    (momentum, short, long)
}

/// The action matrix: verdict × market momentum.
pub fn action_for(verdict: PriceVerdict, momentum: Momentum) -> Action {
    match (verdict, momentum) {
        (PriceVerdict::Underpriced, Momentum::Rising) => Action::RaiseNow,
        (PriceVerdict::Underpriced, Momentum::Falling) => Action::Watch,
        (PriceVerdict::Underpriced, _) => Action::Raise,
        (PriceVerdict::Overpriced, Momentum::Falling) => Action::CutNow,
        (PriceVerdict::Overpriced, Momentum::Rising) => Action::Hold,
        (PriceVerdict::Overpriced, _) => Action::Cut,
        _ => Action::None,
    }
}

/// Priority score — the default "what to fix first" sort key.
///
/// `impact × action-urgency`, where impact is `|delta| × qty` in euros and the
/// urgency weight is 2.0 for now-actions, 1.0 for plain raise/cut, 0.4 for
/// watch/hold, 0 for fair rows. Overpriced rows additionally scale with
/// listing age (dead capital: up to 2× at ≥ 1 year), and stale market data
/// halves the score.
pub fn priority_score(
    delta_abs: f64,
    quantity: i64,
    action: Action,
    age_days: i64,
    stale: bool,
) -> f64 {
    let impact = delta_abs.abs() * quantity.max(0) as f64;
    let mut score = impact * action.weight();
    if matches!(action, Action::CutNow | Action::Cut | Action::Hold) {
        score *= 1.0 + age_days.clamp(0, 365) as f64 / 365.0;
    }
    if stale {
        score *= 0.5;
    }
    score
}

/// Builds the full report for `cards`, resolving each card's market data via
/// `market_of`. `threshold_pct` sets the minimum fair band (e.g. `15.0` for
/// ±15%); per row it is widened to the card's volatility when that is larger.
/// `today` anchors listing-age and staleness computations.
pub fn build_report<F>(
    cards: &[InStockCard],
    threshold_pct: f64,
    today: NaiveDate,
    market_of: F,
) -> MispricingReport
where
    F: Fn(&InStockCard) -> MarketData,
{
    let mut report = MispricingReport::default();

    for card in cards {
        let market = market_of(card);
        let eff_threshold = effective_threshold_pct(threshold_pct, market.volatility_pct);
        let (mut verdict, delta_abs, delta_pct) =
            classify(card.price, market.reference, eff_threshold);

        // Listed below the cheapest listing on the whole market: guaranteed
        // cheapest, so an in-band verdict is still underpriced.
        let below_low =
            verdict != PriceVerdict::NoMarketData && market.low.is_some_and(|l| card.price < l);
        if below_low && verdict == PriceVerdict::Fair {
            verdict = PriceVerdict::Underpriced;
        }

        let (momentum, momentum_short_pct, momentum_long_pct) =
            momentum_of(market.avg1, market.avg7, market.avg30);
        let stale = market
            .price_date
            .is_some_and(|d| (today - d).num_days() > STALE_AFTER_DAYS);
        let age = age_days(card, today);
        let action = action_for(verdict, momentum);
        let qty = card.quantity.max(0);
        let priority = priority_score(delta_abs, qty, action, age, stale);

        match verdict {
            PriceVerdict::Underpriced => {
                report.underpriced_rows += 1;
                report.underpriced_copies += qty;
                // reference is Some here by construction of classify.
                if let Some(m) = market.reference {
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
        match action {
            Action::RaiseNow => report.raise_now_rows += 1,
            Action::CutNow => report.cut_now_rows += 1,
            _ => {}
        }
        if stale {
            report.stale_rows += 1;
        }

        if let Some(m) = market.reference {
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
            market_price: market.reference,
            market_low: market.low,
            delta_abs,
            delta_pct,
            verdict,
            momentum_short_pct,
            momentum_long_pct,
            momentum,
            below_low,
            stale,
            volatility_pct: market.volatility_pct,
            effective_threshold_pct: eff_threshold,
            age_days: age,
            action,
            priority,
        });
    }

    report
}

#[path = "mispricing_tests.rs"]
#[cfg(test)]
mod tests;
