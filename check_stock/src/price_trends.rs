//! Price-movement deltas — pure client-side computation over raw snapshot
//! rows fetched from inventory_sync.
//!
//! The server deliberately never aggregates: `POST /api/price-snapshots`
//! returns the price row in effect on each requested date (an indexed lookup),
//! and everything here — 7/30-day changes, mover ranking — is derived locally.
//! This module is pure (no HTTP, no egui) so it can be tested deterministically.

use crate::aging::age_days;
use crate::api::inventory_sync::{PriceField, PriceFields, PriceHistoryPoint, PriceSnapshot};
use crate::inventory_db::InStockCard;
use chrono::{Days, NaiveDate};
use std::collections::HashMap;

/// Days back from "today" for each snapshot slot. The 7/30-day slots feed the
/// Δ7d/Δ30d columns; the extra 14/60/90-day slots exist so per-card volatility
/// can be estimated locally without any server-side aggregation. Must stay
/// within the server's `MAX_SNAPSHOT_DATES` limit.
pub const SNAPSHOT_DAYS: [u64; 6] = [0, 7, 14, 30, 60, 90];

/// Number of snapshot dates requested per fetch.
pub const SNAPSHOT_SLOT_COUNT: usize = SNAPSHOT_DAYS.len();

/// The dates array sent to `POST /api/price-snapshots`, oldest last.
pub type SnapshotDates = [String; SNAPSHOT_SLOT_COUNT];

/// Slot indices into [`SnapshotSet`] (see [`SNAPSHOT_DAYS`]).
const SLOT_NOW: usize = 0;
const SLOT_WEEK: usize = 1;
const SLOT_MONTH: usize = 3;

/// Percentage change of a price field over the 7- and 30-day windows, plus the
/// current value. `None` means "not enough history", never "no change".
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TrendChange {
    pub current: Option<f64>,
    pub pct_7d: Option<f64>,
    pub pct_30d: Option<f64>,
}

/// Percentage change from `old` to `new`; `None` unless both are present and
/// `old` is a usable (positive) base.
pub fn pct_change(old: Option<f64>, new: Option<f64>) -> Option<f64> {
    match (old, new) {
        (Some(o), Some(n)) if o > 0.0 => Some((n - o) / o * 100.0),
        _ => None,
    }
}

/// Snapshot rows for many products on the [`SNAPSHOT_DAYS`] reference dates,
/// indexed for cheap per-card lookup.
#[derive(Default)]
pub struct SnapshotSet {
    by_product: HashMap<u64, [Option<PriceSnapshot>; SNAPSHOT_SLOT_COUNT]>,
}

impl SnapshotSet {
    /// The dates to request from the server: today and [`SNAPSHOT_DAYS`] back.
    pub fn request_dates(today: NaiveDate) -> SnapshotDates {
        SNAPSHOT_DAYS.map(|days| (today - Days::new(days)).format("%Y-%m-%d").to_string())
    }

    /// Indexes raw server rows by product and requested date. `dates` must be
    /// the same array the request was made with; rows for other dates are
    /// dropped.
    pub fn new(dates: &SnapshotDates, snapshots: Vec<PriceSnapshot>) -> Self {
        let mut by_product: HashMap<u64, [Option<PriceSnapshot>; SNAPSHOT_SLOT_COUNT]> =
            HashMap::new();
        for snap in snapshots {
            let Some(slot) = dates.iter().position(|d| *d == snap.requested_date) else {
                continue;
            };
            let id = snap.id_product;
            by_product.entry(id).or_default()[slot] = Some(snap);
        }
        Self { by_product }
    }

    /// Number of products with at least one snapshot.
    pub fn len(&self) -> usize {
        self.by_product.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_product.is_empty()
    }

    /// Change of `field` for one product, foil-aware.
    ///
    /// A window's change is `None` when either endpoint is missing or when both
    /// requested dates resolved to the same underlying row (i.e. there is no
    /// actual history between them — reporting 0 % would be a lie).
    pub fn change(&self, id_product: u64, field: PriceField, is_foil: bool) -> TrendChange {
        let Some(slots) = self.by_product.get(&id_product) else {
            return TrendChange::default();
        };
        let now = slots[SLOT_NOW].as_ref();
        let current = now.and_then(|s| s.price_for(field, is_foil));
        let window = |slot: usize| -> Option<f64> {
            let (now, past) = (now?, slots[slot].as_ref()?);
            if past.price_date >= now.price_date {
                return None;
            }
            pct_change(past.price_for(field, is_foil), current)
        };
        TrendChange {
            current,
            pct_7d: window(SLOT_WEEK),
            pct_30d: window(SLOT_MONTH),
        }
    }

    /// Relative volatility of `field` for one product: the coefficient of
    /// variation (population std-dev / mean, in percent) across the distinct
    /// history rows behind this product's snapshot slots (~90 days).
    ///
    /// Slots that resolved to the same underlying row (sparse history) count
    /// once. Returns `None` with fewer than 3 distinct values or a
    /// non-positive mean — too little information to call anything volatile.
    pub fn volatility_pct(&self, id_product: u64, field: PriceField, is_foil: bool) -> Option<f64> {
        let slots = self.by_product.get(&id_product)?;
        let mut seen: Vec<&str> = Vec::new();
        let mut values: Vec<f64> = Vec::new();
        for snap in slots.iter().flatten() {
            if seen.contains(&snap.price_date.as_str()) {
                continue;
            }
            seen.push(&snap.price_date);
            if let Some(v) = snap.price_for(field, is_foil) {
                values.push(v);
            }
        }
        if values.len() < 3 {
            return None;
        }
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        if mean <= 0.0 {
            return None;
        }
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
        Some(variance.sqrt() / mean * 100.0)
    }
}

/// Percentage change of the trend price over the last `days` days of one
/// product's history rows (foil-aware), for the per-card detail view.
///
/// The baseline is the most recent row at least `days` days older than the
/// newest row with a value; `None` when the history doesn't reach back that
/// far. `history` must be ordered oldest → newest (as the server returns it).
pub fn roc_from_history(history: &[PriceHistoryPoint], days: u64, is_foil: bool) -> Option<f64> {
    let parse = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    let (last_date, current) = history.iter().rev().find_map(|p| {
        Some((
            parse(&p.price_date)?,
            p.price_for(PriceField::Trend, is_foil)?,
        ))
    })?;
    let cutoff = last_date.checked_sub_days(Days::new(days))?;
    let baseline = history.iter().rev().find_map(|p| {
        let d = parse(&p.price_date)?;
        if d <= cutoff {
            p.price_for(PriceField::Trend, is_foil)
        } else {
            None
        }
    })?;
    pct_change(Some(baseline), Some(current))
}

/// One in-stock card joined with its market movement and listing age —
/// a row of the Price Movers view.
#[derive(Debug, Clone)]
pub struct StockMover {
    pub card: InStockCard,
    pub change: TrendChange,
    /// Days since the card was listed (see [`crate::aging::age_days`]).
    pub age_days: i64,
}

/// Joins in-stock cards with their snapshot-derived changes.
///
/// Cards are skipped when their cardmarket ID doesn't parse, when the current
/// `field` value is below `min_price` (bulk noise), or when neither window has
/// enough history to compute a change.
pub fn build_stock_movers(
    cards: &[InStockCard],
    snapshots: &SnapshotSet,
    field: PriceField,
    today: NaiveDate,
    min_price: f64,
) -> Vec<StockMover> {
    cards
        .iter()
        .filter_map(|card| {
            let id = card.cardmarket_id.parse::<u64>().ok()?;
            let change = snapshots.change(id, field, card.is_foil);
            let current = change.current?;
            if current < min_price {
                return None;
            }
            if change.pct_7d.is_none() && change.pct_30d.is_none() {
                return None;
            }
            Some(StockMover {
                card: card.clone(),
                change,
                age_days: age_days(card, today),
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "price_trends_tests.rs"]
mod tests;
