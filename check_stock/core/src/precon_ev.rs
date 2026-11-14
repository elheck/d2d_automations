//! Precon EV: expected singles value of Commander precon decks.
//!
//! Pure logic behind the Precon EV screen — no egui, no network. The catalog
//! of precons and every price come from the inventory_sync price tracker;
//! decklists come from MTGJSON (see [`crate::precon_loader`]). This module
//! joins them:
//!
//! - **EV** = Σ (unit price × copies) over card copies whose unit price is at
//!   least the bulk cutoff — cheaper cards rarely sell, so they are excluded.
//! - **Top 5** = the five most expensive card copies, regardless of the
//!   cutoff. A high Top 5 means the deck sells well when opened for singles.
//! - Display/oversized commanders are excluded from both.
//! - Foil cards use the foil price; if the tracker has no foil price the
//!   non-foil price is used and flagged. Never the other way round, which
//!   would inflate EV. Prices ≤ 0 count as missing.
//! - With `foil_needs_sales`, a foil that has not sold in the last
//!   [`FOIL_LOOKBACK_DAYS`] days is also valued at the non-foil price: a
//!   foil premium nobody pays (typical for Collector's Editions) is not value.
//! - **Liquid EV** weights each counted copy by how often it sells: full
//!   weight from a sale on [`LIQUID_FULL_SHARE`] of recent days, less below.

use crate::api::inventory_sync::{
    LatestPrice, PriceField, PriceFields, PriceSnapshot, ProductSearchResult,
};
use crate::api::mtgjson::{DeckBoard, DeckCard, DeckResolution, PreconDeck, SealedIndex};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// Tracker search term for the precon catalog. Every precon product name
/// contains "Commander", so this returns a superset of the catalog.
pub const CATALOG_QUERY: &str = "Commander";
/// Result limit for the catalog search (today ~550 rows match).
pub const CATALOG_LIMIT: usize = 5_000;
/// Cardmarket category of preconstructed decks.
pub const PRECON_CATEGORY: &str = "Magic Intropack";
/// How many of the most expensive copies make up the "Top 5" value.
pub const TOP_N: usize = 5;
/// Default bulk cutoff in €.
pub const DEFAULT_CUTOFF_EUR: f64 = 0.50;
/// History points need this share of priced copies to be plotted; below it
/// (e.g. before the deck was released) the point is a gap.
pub const MIN_HISTORY_COVERAGE: f64 = 0.90;
/// How far back the EV history chart reaches.
pub const HISTORY_WEEKS: i64 = 52;
/// Price columns Cardmarket publishes for sealed products. Its price guide
/// has no 1/7/30-day averages for them, so those can only price cards.
pub const SEALED_FIELDS: [PriceField; 3] = [PriceField::Trend, PriceField::Low, PriceField::Avg];
/// A sealed price more than this factor below the cheapest current offer is
/// stale (thin market) — the deck cannot be bought anywhere near it, so the
/// EV/Sealed ratio is overstated.
pub const STALE_SEALED_FACTOR: f64 = 1.25;
/// Days of daily sales history behind the liquidity signal.
pub const SALES_DAYS: u32 = 14;
/// How far back "did this foil sell at all" looks (foil premium gate). One
/// extra snapshot date, so the window costs 16 dates = 2 tracker requests.
pub const FOIL_LOOKBACK_DAYS: u32 = 49;
/// A card that sold on at least this share of observed days counts fully in
/// Liquid EV; slower cards count proportionally less, unsold ones not at all.
pub const LIQUID_FULL_SHARE: f64 = 0.5;
/// Cards below this price (trend, in the finish the deck contains) get no
/// sales history: Liquid EV treats them as unsellable. Equal to the default
/// bulk cutoff, so nothing counted in EV by default goes unchecked; halves
/// the load on the tracker compared with checking every card.
pub const SALES_MIN_PRICE_EUR: f64 = 0.50;
/// A day on which fewer than this share of a batch's cards had their sales
/// averages move is a Cardmarket data freeze, not a quiet market, and is not
/// observed. Cardmarket skips averages updates for newer products on some
/// days (and froze them ~24 Sep–3 Oct 2026) while older ones keep updating;
/// batches are grouped by product id, so they follow that split. A batch
/// straddling it may miss a freeze, costing its frozen cards a sale day.
const FROZEN_MAX_SHARE: f64 = 0.05;
/// Fewer cards than this cannot tell a freeze from a quiet market; then
/// every day counts.
const FROZEN_MIN_CARDS: usize = 20;

// ── Catalog ──────────────────────────────────────────────────────────────────

/// Keeps Commander precon products (category + "commander" in the name,
/// case-insensitive) and drops duplicate ids.
pub fn filter_precon_catalog(rows: Vec<ProductSearchResult>) -> Vec<ProductSearchResult> {
    let mut seen = HashSet::new();
    rows.into_iter()
        .filter(|p| {
            p.category_name == PRECON_CATEGORY
                && p.name.to_lowercase().contains("commander")
                && seen.insert(p.id_product)
        })
        .collect()
}

/// True when a search hit the limit, i.e. results may be cut off.
pub fn catalog_truncated(len: usize, limit: usize) -> bool {
    len >= limit
}

/// Cardmarket names escape quotes by doubling them (`""Animated Army""`).
pub fn display_name(raw: &str) -> String {
    raw.replace("\"\"", "\"").trim().to_string()
}

// ── Prices ───────────────────────────────────────────────────────────────────

/// A card's unit price and whether the non-foil price stood in for a foil.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitPrice {
    pub eur: f64,
    pub foil_fallback: bool,
}

fn positive(price: Option<f64>) -> Option<f64> {
    price.filter(|p| p.is_finite() && *p > 0.0)
}

/// Unit price of one card copy. Foil falls back to non-foil (flagged);
/// non-foil never falls back to foil.
pub fn unit_price<P: PriceFields>(
    row: Option<&P>,
    field: PriceField,
    is_foil: bool,
) -> Option<UnitPrice> {
    let row = row?;
    if is_foil {
        if let Some(eur) = positive(row.price_for(field, true)) {
            return Some(UnitPrice {
                eur,
                foil_fallback: false,
            });
        }
        return positive(row.price_for(field, false)).map(|eur| UnitPrice {
            eur,
            foil_fallback: true,
        });
    }
    positive(row.price_for(field, false)).map(|eur| UnitPrice {
        eur,
        foil_fallback: false,
    })
}

/// Price of the sealed product (non-foil columns only).
pub fn sealed_price<P: PriceFields>(row: Option<&P>, field: PriceField) -> Option<f64> {
    positive(row?.price_for(field, false))
}

/// The cheapest offer when it is more than [`STALE_SEALED_FACTOR`] above the
/// sealed price used — i.e. that price is stale and nobody sells at it.
pub fn stale_below_offer(sealed: Option<f64>, cheapest_offer: Option<f64>) -> Option<f64> {
    match (sealed, cheapest_offer) {
        (Some(price), Some(low)) if low > price * STALE_SEALED_FACTOR => Some(low),
        _ => None,
    }
}

// ── Sales ────────────────────────────────────────────────────────────────────

/// Recent sales activity of a card, per finish. Market-wide: all sellers,
/// countries and languages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SalesStats {
    /// Observed days (of the last [`SALES_DAYS`]) with at least one sale.
    pub nonfoil_days: u8,
    pub foil_days: u8,
    /// Days with fresh, unfrozen data (tracker gaps and freezes excluded).
    pub observed_days: u8,
    /// Sold at all in the last [`FOIL_LOOKBACK_DAYS`].
    pub nonfoil_recent: bool,
    pub foil_recent: bool,
}

/// Card id → sales stats. A card missing from a loaded map had no sales, or
/// was below [`SALES_MIN_PRICE_EUR`] and not checked.
pub type SalesMap = HashMap<u64, SalesStats>;

/// The snapshot dates behind [`SalesStats`]: one lookback date plus every
/// day of the last [`SALES_DAYS`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesWindow {
    pub lookback: String,
    /// `SALES_DAYS + 1` consecutive days ending on the window's last day.
    pub daily: Vec<String>,
}

impl SalesWindow {
    /// The window ending on `today`.
    pub fn ending(today: NaiveDate) -> Self {
        let day = |back: u32| (today - chrono::Duration::days(i64::from(back))).format("%Y-%m-%d");
        Self {
            lookback: day(FOIL_LOOKBACK_DAYS).to_string(),
            daily: (0..=SALES_DAYS).rev().map(|d| day(d).to_string()).collect(),
        }
    }

    /// All dates to request, ascending.
    pub fn dates(&self) -> Vec<String> {
        std::iter::once(self.lookback.clone())
            .chain(self.daily.iter().cloned())
            .collect()
    }
}

/// A card's snapshot rows: id, lookback row, daily rows.
type CardRows<'a> = (
    u64,
    Option<&'a PriceSnapshot>,
    Vec<Option<&'a PriceSnapshot>>,
);

/// The sales averages of a snapshot row, non-foil then foil.
fn sales_averages(s: &PriceSnapshot) -> [Option<f64>; 6] {
    [
        s.avg1,
        s.avg7,
        s.avg30,
        s.avg1_foil,
        s.avg7_foil,
        s.avg30_foil,
    ]
}

/// Whether day `k` of a card's daily rows has fresh data: both rows exist
/// and come from different tracker days (a tracker gap repeats the row).
fn fresh_day(rows: &[Option<&PriceSnapshot>], k: usize) -> bool {
    matches!((rows[k - 1], rows[k]), (Some(a), Some(b)) if a.price_date != b.price_date)
}

/// Counts sale days from daily snapshots (grouped by `requested_date`).
///
/// Cardmarket's 1-day average is carried forward on days without a sale, so
/// a day on which `avg1` moved had at least one sale; sales at exactly the
/// previous average are missed, so this is a lower bound. Days on which
/// (almost) no card's averages moved are a data freeze and not observed.
/// "Recent" also compares today's `avg1` with the lookback date's.
pub fn sales_stats(snapshots: &[PriceSnapshot], window: &SalesWindow) -> SalesMap {
    let mut rows: HashMap<u64, HashMap<&str, &PriceSnapshot>> = HashMap::new();
    for s in snapshots {
        rows.entry(s.id_product)
            .or_default()
            .insert(s.requested_date.as_str(), s);
    }
    let cards: Vec<CardRows> = rows
        .into_iter()
        .map(|(id, by_date)| {
            let daily = window
                .daily
                .iter()
                .map(|d| by_date.get(d.as_str()).copied())
                .collect();
            (id, by_date.get(window.lookback.as_str()).copied(), daily)
        })
        .collect();

    // Day k (from daily[k-1] to daily[k]) is frozen when the batch is big
    // enough to judge and almost no card's sales averages moved.
    let days = window.daily.len().saturating_sub(1);
    let frozen: Vec<bool> = (1..=days)
        .map(|k| {
            let (mut with_data, mut moved) = (0usize, 0usize);
            for (_, _, daily) in &cards {
                if !fresh_day(daily, k) {
                    continue;
                }
                let (a, b) = (
                    sales_averages(daily[k - 1].unwrap()),
                    sales_averages(daily[k].unwrap()),
                );
                if b.iter().any(Option::is_some) {
                    with_data += 1;
                    moved += usize::from(a != b);
                }
            }
            with_data >= FROZEN_MIN_CARDS && (moved as f64) < FROZEN_MAX_SHARE * with_data as f64
        })
        .collect();

    cards
        .into_iter()
        .map(|(id, lookback, daily)| {
            let observed: Vec<usize> = (1..=days)
                .filter(|&k| !frozen[k - 1] && fresh_day(&daily, k))
                .collect();
            let finish = |pick: fn(&PriceSnapshot) -> Option<f64>| -> (u8, bool) {
                let value = |row: Option<&PriceSnapshot>| row.and_then(|s| positive(pick(s)));
                let sale_days = observed
                    .iter()
                    .filter(|&&k| {
                        let now = value(daily[k]);
                        now.is_some() && now != value(daily[k - 1])
                    })
                    .count();
                let latest = value(daily.last().copied().flatten());
                let recent = sale_days > 0 || (latest.is_some() && latest != value(lookback));
                (u8::try_from(sale_days).unwrap_or(u8::MAX), recent)
            };
            let (nonfoil_days, nonfoil_recent) = finish(|s| s.avg1);
            let (foil_days, foil_recent) = finish(|s| s.avg1_foil);
            let stats = SalesStats {
                nonfoil_days,
                foil_days,
                observed_days: u8::try_from(observed.len()).unwrap_or(u8::MAX),
                nonfoil_recent,
                foil_recent,
            };
            (id, stats)
        })
        .collect()
}

/// Card ids worth fetching sales history for: deck cards priced at least
/// [`SALES_MIN_PRICE_EUR`] in the finish the deck contains (foils also by
/// their non-foil price, which the foil gate may fall back to). Display
/// cards are skipped. Sorted, deduplicated.
pub fn sales_candidates(
    decks: &HashMap<String, PreconDeck>,
    prices: &HashMap<u64, LatestPrice>,
) -> Vec<u64> {
    let mut out: Vec<u64> = decks
        .values()
        .flat_map(|d| d.cards.iter())
        .filter(|c| c.board != DeckBoard::Display)
        .filter_map(|c| {
            let p = prices.get(&c.mcm_id?)?;
            let finishes = if c.is_foil {
                [p.trend, p.trend_foil]
            } else {
                [p.trend, None]
            };
            finishes
                .into_iter()
                .flatten()
                .any(|v| v >= SALES_MIN_PRICE_EUR)
                .then_some(c.mcm_id?)
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Liquid EV weight of a card: its share of observed days with a sale,
/// scaled so that [`LIQUID_FULL_SHARE`] or more counts fully (0 when
/// nothing was observed).
pub fn liquidity_weight(sale_days: u8, observed_days: u8) -> f64 {
    if observed_days == 0 {
        return 0.0;
    }
    let share = f64::from(sale_days) / f64::from(observed_days);
    (share / LIQUID_FULL_SHARE).clamp(0.0, 1.0)
}

/// Unit price under the EV rules: [`unit_price`], except that with
/// `foil_needs_sales` a foil that did not sell in the last
/// [`FOIL_LOOKBACK_DAYS`] is valued at its non-foil price (flagged as a foil
/// fallback). Foil-only printings without a non-foil price keep their foil
/// price. `sales` is `None` when no sales history is loaded — then nothing
/// is gated.
pub fn card_unit<P: PriceFields>(
    row: Option<&P>,
    sales: Option<SalesStats>,
    params: EvParams,
    is_foil: bool,
) -> Option<UnitPrice> {
    let unsold_foil = is_foil && params.foil_needs_sales && sales.is_some_and(|s| !s.foil_recent);
    if unsold_foil {
        if let Some(eur) = positive(row?.price_for(params.field, false)) {
            return Some(UnitPrice {
                eur,
                foil_fallback: true,
            });
        }
    }
    unit_price(row, params.field, is_foil)
}

// ── EV ───────────────────────────────────────────────────────────────────────

/// One decklist line with its unit price (if known).
#[derive(Debug, Clone, PartialEq)]
pub struct CardValue<'a> {
    pub card: &'a DeckCard,
    /// Index into the decks passed to [`value_cards`] (bundles have several).
    pub deck_idx: usize,
    pub unit: Option<UnitPrice>,
    /// Sale days of the finish the unit price is based on; `None` without
    /// sales history or price.
    pub sale_days: Option<u8>,
    /// Days observed for `sale_days` (0 = card not checked).
    pub observed_days: Option<u8>,
    /// Liquid EV weight from `sale_days` ([`liquidity_weight`]).
    pub liquidity: Option<f64>,
}

impl CardValue<'_> {
    /// Unit price × copies, if priced.
    pub fn line_total(&self) -> Option<f64> {
        self.unit.map(|u| u.eur * f64::from(self.card.count))
    }
}

/// Prices every non-display card of `decks` via `lookup` (Cardmarket id →
/// price row). `sales` (if loaded) gates foil premiums and drives the sell
/// probability.
pub fn value_cards<'a, 'p, P, F>(
    decks: &[&'a PreconDeck],
    lookup: F,
    sales: Option<&SalesMap>,
    params: EvParams,
) -> Vec<CardValue<'a>>
where
    P: PriceFields + 'p,
    F: Fn(u64) -> Option<&'p P>,
{
    let mut out = Vec::new();
    for (deck_idx, deck) in decks.iter().enumerate() {
        for card in deck.cards.iter().filter(|c| c.board != DeckBoard::Display) {
            let stats = sales
                .zip(card.mcm_id)
                .map(|(map, id)| map.get(&id).copied().unwrap_or_default());
            let unit = card_unit(card.mcm_id.and_then(&lookup), stats, params, card.is_foil);
            let sale_days = stats.zip(unit).map(|(s, u)| {
                if card.is_foil && !u.foil_fallback {
                    s.foil_days
                } else {
                    s.nonfoil_days
                }
            });
            let observed_days = sale_days.and(stats.map(|s| s.observed_days));
            out.push(CardValue {
                card,
                deck_idx,
                unit,
                sale_days,
                observed_days,
                liquidity: sale_days
                    .zip(observed_days)
                    .map(|(d, o)| liquidity_weight(d, o)),
            });
        }
    }
    out
}

/// Indices into `values`, most expensive unit price first; unpriced last.
pub fn ranked_by_unit(values: &[CardValue]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..values.len()).collect();
    idx.sort_by(|&a, &b| {
        let ua = values[a].unit.map(|u| u.eur);
        let ub = values[b].unit.map(|u| u.eur);
        cmp_none_last(ua, ub, true, f64::total_cmp)
            .then_with(|| values[a].card.name.cmp(&values[b].card.name))
    });
    idx
}

/// Sum of the `n` most expensive card copies (a line with count 3 fills up
/// to 3 slots). Ignores the bulk cutoff and unpriced cards.
pub fn top_n_copies(values: &[CardValue], n: usize) -> f64 {
    let mut remaining = n as u32;
    let mut sum = 0.0;
    for i in ranked_by_unit(values) {
        let Some(unit) = values[i].unit else { break };
        if remaining == 0 {
            break;
        }
        let take = values[i].card.count.min(remaining);
        sum += unit.eur * f64::from(take);
        remaining -= take;
    }
    sum
}

/// Indices of the lines that contribute at least one copy to the top `n`.
pub fn top_n_lines(values: &[CardValue], n: usize) -> HashSet<usize> {
    let mut remaining = n as u32;
    let mut out = HashSet::new();
    for i in ranked_by_unit(values) {
        if remaining == 0 || values[i].unit.is_none() {
            break;
        }
        remaining -= values[i].card.count.min(remaining);
        out.insert(i);
    }
    out
}

/// Aggregated value of a deck (or a bundle of decks).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeckEv {
    /// Σ line totals of copies priced at or above the cutoff.
    pub ev: f64,
    /// Σ line totals of all priced copies (no cutoff).
    pub gross: f64,
    /// Sum of the [`TOP_N`] most expensive copies.
    pub top5: f64,
    pub total_copies: u32,
    pub priced_copies: u32,
    /// Priced copies counted in `ev` (unit ≥ cutoff).
    pub counted_copies: u32,
    pub below_cutoff_copies: u32,
    pub unpriced_copies: u32,
    pub foil_fallback_copies: u32,
    /// Σ line × liquidity weight over the copies counted in `ev`; `None`
    /// without sales history.
    pub liquid: Option<f64>,
}

impl DeckEv {
    /// Share of copies that have a price (0.0–1.0).
    pub fn coverage(&self) -> f64 {
        if self.total_copies == 0 {
            0.0
        } else {
            f64::from(self.priced_copies) / f64::from(self.total_copies)
        }
    }
}

/// Aggregates priced lines; a copy counts toward EV when `unit >= cutoff`.
pub fn summarize(values: &[CardValue], cutoff: f64) -> DeckEv {
    let mut ev = DeckEv {
        top5: top_n_copies(values, TOP_N),
        ..DeckEv::default()
    };
    let mut liquid = 0.0;
    for v in values {
        let count = v.card.count;
        ev.total_copies += count;
        let Some(unit) = v.unit else {
            ev.unpriced_copies += count;
            continue;
        };
        let line = unit.eur * f64::from(count);
        ev.priced_copies += count;
        ev.gross += line;
        if unit.foil_fallback {
            ev.foil_fallback_copies += count;
        }
        if unit.eur >= cutoff {
            ev.ev += line;
            ev.counted_copies += count;
            liquid += line * v.liquidity.unwrap_or(0.0);
        } else {
            ev.below_cutoff_copies += count;
        }
    }
    ev.liquid = values
        .iter()
        .any(|v| v.sale_days.is_some())
        .then_some(liquid);
    ev
}

/// EV ÷ sealed price, when the sealed price is known.
pub fn ev_ratio(ev: f64, sealed: Option<f64>) -> Option<f64> {
    sealed.filter(|s| *s > 0.0).map(|s| ev / s)
}

// ── Ranking ──────────────────────────────────────────────────────────────────

/// Everything a load produced; rows are rebuilt from it without refetching.
#[derive(Debug, Clone, Default)]
pub struct PreconData {
    /// Precon products from the tracker (always fetched live).
    pub catalog: Vec<ProductSearchResult>,
    pub catalog_truncated: bool,
    /// Sealed product → deck keys (cached; see `cache::deck_cache`).
    pub index: SealedIndex,
    pub index_fetched_at_unix: i64,
    /// MTGJSON was unreachable and an older cached index was used.
    pub index_stale: bool,
    /// MTGJSON version of the (live) DeckList.
    pub deck_list_version: Option<String>,
    /// Catalog product id → resolved deck files.
    pub resolved: HashMap<u64, DeckResolution>,
    /// Deck `fileName` → decklist.
    pub decks: HashMap<String, PreconDeck>,
    /// Deck `fileName` → why it could not be loaded.
    pub deck_errors: HashMap<String, String>,
    /// Latest tracker price per product id (cards and sealed products).
    pub prices: HashMap<u64, LatestPrice>,
    /// Sales stats of the deck cards (cached per day); `None` if the sales
    /// history could not be fetched.
    pub sales: Option<SalesMap>,
    pub sales_error: Option<String>,
}

impl PreconData {
    /// The decklists of a product, if all of them are loaded.
    pub fn decks_for(&self, id_product: u64) -> Option<Vec<&PreconDeck>> {
        match self.resolved.get(&id_product)? {
            DeckResolution::Ready(refs) => {
                refs.iter().map(|r| self.decks.get(&r.file_name)).collect()
            }
            DeckResolution::Pending(_) => None,
        }
    }
}

/// Why a product has (or lacks) an EV.
#[derive(Debug, Clone, PartialEq)]
pub enum PreconStatus {
    /// All decklists loaded; `decks` > 1 for bundles.
    Ready { decks: usize },
    /// MTGJSON has no sealed product with this Cardmarket id (yet).
    NotOnMtgjson,
    /// MTGJSON knows the product but has not published these decklists.
    DecklistPending(Vec<String>),
    /// A decklist download failed.
    DeckUnavailable(String),
}

/// Pricing parameters chosen in the UI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvParams {
    /// Price column for the cards (any of the six).
    pub field: PriceField,
    /// Price column for the sealed product (one of [`SEALED_FIELDS`]).
    pub sealed_field: PriceField,
    pub cutoff: f64,
    /// Value a foil at its non-foil price unless the foil sold recently.
    pub foil_needs_sales: bool,
}

/// One row of the ranking table.
#[derive(Debug, Clone, PartialEq)]
pub struct PreconRow {
    pub id_product: u64,
    pub name: String,
    /// Expansion name from the tracker, else the MTGJSON set code.
    pub set_label: String,
    pub release_date: Option<String>,
    pub status: PreconStatus,
    /// Sealed price at [`EvParams::sealed_field`].
    pub sealed_price: Option<f64>,
    /// Cheapest current offer for the sealed product (Cardmarket `low`).
    pub sealed_low: Option<f64>,
    pub ev: Option<DeckEv>,
    pub ratio: Option<f64>,
}

impl PreconRow {
    pub fn is_ready(&self) -> bool {
        matches!(self.status, PreconStatus::Ready { .. })
    }

    /// The cheapest offer, when the sealed price used is stale below it.
    pub fn stale_sealed(&self) -> Option<f64> {
        stale_below_offer(self.sealed_price, self.sealed_low)
    }

    /// Short note for the ranking table plus optional hover detail.
    pub fn notes(&self) -> (String, Option<String>) {
        match &self.status {
            PreconStatus::Ready { decks } => {
                let mut parts = Vec::new();
                let mut hover = None;
                if let (Some(low), Some(price)) = (self.stale_sealed(), self.sealed_price) {
                    parts.push(format!("⚠ cheapest offer €{low:.2}"));
                    hover = Some(format!(
                        "Nobody sells near the sealed price used (€{price:.2}) — the cheapest \
                         offer is €{low:.2}. Thin market: EV/Sealed is overstated."
                    ));
                }
                if *decks > 1 {
                    parts.push(format!("bundle of {decks} decks"));
                }
                if let Some(ev) = &self.ev {
                    if ev.foil_fallback_copies > 0 {
                        parts.push(format!("{} foil→non-foil", ev.foil_fallback_copies));
                    }
                    if ev.unpriced_copies > 0 {
                        parts.push(format!("{} unpriced", ev.unpriced_copies));
                    }
                }
                (parts.join(" · "), hover)
            }
            PreconStatus::NotOnMtgjson => ("not on MTGJSON yet".to_string(), None),
            PreconStatus::DecklistPending(names) => (
                "decklist not published yet".to_string(),
                Some(names.join(", ")),
            ),
            PreconStatus::DeckUnavailable(err) => {
                ("decklist download failed".to_string(), Some(err.clone()))
            }
        }
    }
}

/// Builds one ranking row per catalog product.
pub fn build_rows(data: &PreconData, params: EvParams) -> Vec<PreconRow> {
    data.catalog
        .iter()
        .map(|p| {
            let id = p.id_product;
            let status = match data.resolved.get(&id) {
                None => PreconStatus::NotOnMtgjson,
                Some(DeckResolution::Pending(names)) => {
                    PreconStatus::DecklistPending(names.clone())
                }
                Some(DeckResolution::Ready(refs)) => {
                    match refs.iter().find(|r| !data.decks.contains_key(&r.file_name)) {
                        Some(missing) => PreconStatus::DeckUnavailable(
                            data.deck_errors
                                .get(&missing.file_name)
                                .cloned()
                                .unwrap_or_else(|| "decklist not loaded".to_string()),
                        ),
                        None => PreconStatus::Ready { decks: refs.len() },
                    }
                }
            };
            let release_date = match data.resolved.get(&id) {
                Some(DeckResolution::Ready(refs)) => {
                    refs.iter().find_map(|r| r.release_date.clone())
                }
                _ => None,
            };
            let set_label = p
                .expansion_name
                .clone()
                .or_else(|| data.index.products.get(&id).map(|s| s.set_code.clone()))
                .unwrap_or_default();
            let sealed = sealed_price(data.prices.get(&id), params.sealed_field);
            let sealed_low = sealed_price(data.prices.get(&id), PriceField::Low);
            let ev = data.decks_for(id).map(|decks| {
                let values =
                    value_cards(&decks, |i| data.prices.get(&i), data.sales.as_ref(), params);
                summarize(&values, params.cutoff)
            });
            let ratio = ev.as_ref().and_then(|e| ev_ratio(e.ev, sealed));
            PreconRow {
                id_product: id,
                name: display_name(&p.name),
                set_label,
                release_date,
                status,
                sealed_price: sealed,
                sealed_low,
                ev,
                ratio,
            }
        })
        .collect()
}

/// Columns the ranking can be sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreconSort {
    /// EV ÷ sealed price.
    #[default]
    Ratio,
    Ev,
    /// Liquid EV (sales-frequency weighted).
    Liquid,
    Top5,
    Sealed,
    Coverage,
    Release,
    Name,
}

/// Compares two optional keys; `None` sorts last in both directions.
fn cmp_none_last<T>(
    a: Option<T>,
    b: Option<T>,
    desc: bool,
    cmp: impl Fn(&T, &T) -> Ordering,
) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => {
            let ord = cmp(&x, &y);
            if desc {
                ord.reverse()
            } else {
                ord
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Sorts rows in place; rows without a value for the column go last, ties
/// break by name.
pub fn sort_rows(rows: &mut [PreconRow], sort: PreconSort, desc: bool) {
    let num = |r: &PreconRow| -> Option<f64> {
        match sort {
            PreconSort::Ratio => r.ratio,
            PreconSort::Ev => r.ev.as_ref().map(|e| e.ev),
            PreconSort::Liquid => r.ev.as_ref().and_then(|e| e.liquid),
            PreconSort::Top5 => r.ev.as_ref().map(|e| e.top5),
            PreconSort::Sealed => r.sealed_price,
            PreconSort::Coverage => r.ev.as_ref().map(DeckEv::coverage),
            PreconSort::Release | PreconSort::Name => None,
        }
    };
    rows.sort_by(|a, b| {
        let ord = match sort {
            PreconSort::Name => cmp_none_last(
                Some(a.name.to_lowercase()),
                Some(b.name.to_lowercase()),
                desc,
                |x, y| x.cmp(y),
            ),
            PreconSort::Release => cmp_none_last(
                a.release_date.as_deref(),
                b.release_date.as_deref(),
                desc,
                |x, y| x.cmp(y),
            ),
            _ => cmp_none_last(num(a), num(b), desc, f64::total_cmp),
        };
        ord.then_with(|| a.name.cmp(&b.name))
    });
}

/// Rows matching the text filter (name or set, case-insensitive); with
/// `hide_no_decklist`, only rows that have an EV.
pub fn visible_rows<'a>(
    rows: &'a [PreconRow],
    filter: &str,
    hide_no_decklist: bool,
) -> Vec<&'a PreconRow> {
    let needle = filter.trim().to_lowercase();
    rows.iter()
        .filter(|r| !hide_no_decklist || r.is_ready())
        .filter(|r| {
            needle.is_empty()
                || r.name.to_lowercase().contains(&needle)
                || r.set_label.to_lowercase().contains(&needle)
        })
        .collect()
}

/// Every product id whose price is needed: catalog (sealed) ids plus all
/// card ids of the loaded decks. Sorted, deduplicated.
pub fn price_ids(catalog: &[ProductSearchResult], decks: &HashMap<String, PreconDeck>) -> Vec<u64> {
    let mut ids: Vec<u64> = catalog
        .iter()
        .map(|p| p.id_product)
        .chain(decks.values().flat_map(PreconDeck::card_ids))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

// ── History ──────────────────────────────────────────────────────────────────

/// First history date: the deck's release date, but at most
/// [`HISTORY_WEEKS`] back from `today`.
pub fn history_start(release: Option<&str>, today: NaiveDate) -> NaiveDate {
    let window = today - chrono::Duration::weeks(HISTORY_WEEKS);
    release
        .and_then(|r| NaiveDate::parse_from_str(r, "%Y-%m-%d").ok())
        .map_or(window, |r| r.max(window))
}

/// Weekly ISO dates from `start` to `today`, ascending and ending on `today`.
pub fn weekly_dates(start: NaiveDate, today: NaiveDate) -> Vec<String> {
    let mut dates = Vec::new();
    let mut d = today;
    while d >= start {
        dates.push(d.format("%Y-%m-%d").to_string());
        d -= chrono::Duration::days(7);
    }
    dates.reverse();
    dates
}

/// One point of the EV-over-time chart.
#[derive(Debug, Clone, PartialEq)]
pub struct EvPoint {
    pub date: String,
    /// `None` when fewer than [`MIN_HISTORY_COVERAGE`] of copies were priced.
    pub ev: Option<f64>,
    pub top5: Option<f64>,
    pub sealed: Option<f64>,
    pub coverage: f64,
}

/// EV, Top 5 and sealed price per requested date, from snapshot rows
/// (grouped by their `requested_date`). Foil gating uses today's `sales`.
pub fn ev_history(
    decks: &[&PreconDeck],
    sealed_id: u64,
    snapshots: &[PriceSnapshot],
    dates: &[String],
    sales: Option<&SalesMap>,
    params: EvParams,
) -> Vec<EvPoint> {
    let mut by_date: HashMap<&str, HashMap<u64, &PriceSnapshot>> = HashMap::new();
    for s in snapshots {
        by_date
            .entry(s.requested_date.as_str())
            .or_default()
            .insert(s.id_product, s);
    }
    dates
        .iter()
        .map(|date| {
            let rows = by_date.get(date.as_str());
            let lookup = |id: u64| rows.and_then(|m| m.get(&id).copied());
            let values = value_cards(decks, lookup, sales, params);
            let summary = summarize(&values, params.cutoff);
            let coverage = summary.coverage();
            let enough = coverage >= MIN_HISTORY_COVERAGE;
            EvPoint {
                date: date.clone(),
                ev: enough.then_some(summary.ev),
                top5: enough.then_some(summary.top5),
                sealed: sealed_price(lookup(sealed_id), params.sealed_field),
                coverage,
            }
        })
        .collect()
}

/// Drops leading points that have neither an EV nor a sealed price (dates
/// before the tracker has data for the deck).
pub fn trim_leading_gaps(points: Vec<EvPoint>) -> Vec<EvPoint> {
    let first = points
        .iter()
        .position(|p| p.ev.is_some() || p.sealed.is_some())
        .unwrap_or(points.len());
    points.into_iter().skip(first).collect()
}

#[cfg(test)]
#[path = "precon_ev_tests.rs"]
mod tests;
