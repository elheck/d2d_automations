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

use crate::api::inventory_sync::{
    LatestPrice, PriceField, PriceFields, PriceSnapshot, ProductSearchResult,
};
use crate::api::mtgjson::{DeckBoard, DeckCard, DeckResolution, PreconDeck, SealedIndex};
use chrono::NaiveDate;
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

// ── EV ───────────────────────────────────────────────────────────────────────

/// One decklist line with its unit price (if known).
#[derive(Debug, Clone, PartialEq)]
pub struct CardValue<'a> {
    pub card: &'a DeckCard,
    /// Index into the decks passed to [`value_cards`] (bundles have several).
    pub deck_idx: usize,
    pub unit: Option<UnitPrice>,
}

impl CardValue<'_> {
    /// Unit price × copies, if priced.
    pub fn line_total(&self) -> Option<f64> {
        self.unit.map(|u| u.eur * f64::from(self.card.count))
    }
}

/// Prices every non-display card of `decks` via `lookup` (Cardmarket id →
/// price row).
pub fn value_cards<'a, 'p, P, F>(
    decks: &[&'a PreconDeck],
    lookup: F,
    field: PriceField,
) -> Vec<CardValue<'a>>
where
    P: PriceFields + 'p,
    F: Fn(u64) -> Option<&'p P>,
{
    let mut out = Vec::new();
    for (deck_idx, deck) in decks.iter().enumerate() {
        for card in deck.cards.iter().filter(|c| c.board != DeckBoard::Display) {
            out.push(CardValue {
                card,
                deck_idx,
                unit: card
                    .mcm_id
                    .and_then(|id| unit_price(lookup(id), field, card.is_foil)),
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
        } else {
            ev.below_cutoff_copies += count;
        }
    }
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
    pub field: PriceField,
    pub cutoff: f64,
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
    pub sealed_price: Option<f64>,
    pub ev: Option<DeckEv>,
    pub ratio: Option<f64>,
}

impl PreconRow {
    pub fn is_ready(&self) -> bool {
        matches!(self.status, PreconStatus::Ready { .. })
    }

    /// Short note for the ranking table plus optional hover detail.
    pub fn notes(&self) -> (String, Option<String>) {
        match &self.status {
            PreconStatus::Ready { decks } => {
                let mut parts = Vec::new();
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
                (parts.join(" · "), None)
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
            let sealed = sealed_price(data.prices.get(&id), params.field);
            let ev = data.decks_for(id).map(|decks| {
                let values = value_cards(&decks, |i| data.prices.get(&i), params.field);
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
/// (grouped by their `requested_date`).
pub fn ev_history(
    decks: &[&PreconDeck],
    sealed_id: u64,
    snapshots: &[PriceSnapshot],
    dates: &[String],
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
            let values = value_cards(decks, lookup, params.field);
            let summary = summarize(&values, params.cutoff);
            let coverage = summary.coverage();
            let enough = coverage >= MIN_HISTORY_COVERAGE;
            EvPoint {
                date: date.clone(),
                ev: enough.then_some(summary.ev),
                top5: enough.then_some(summary.top5),
                sealed: sealed_price(lookup(sealed_id), params.field),
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
