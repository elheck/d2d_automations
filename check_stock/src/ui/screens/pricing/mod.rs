//! Repricing screen: reprice the whole stock by one ordered ruleset.
//!
//! Three inputs feed one live result:
//! 1. **Stock**: the Cardmarket inventory export (synced into the inventory DB
//!    like on every screen that loads it), joined with each listing's age and
//!    sales from that DB.
//! 2. **Market prices**: the latest rows from the inventory_sync tracker, or
//!    a Cardmarket price-guide download.
//! 3. **Rules**: a [`Ruleset`] edited in the left panel and saved
//!    automatically between sessions.
//!
//! Every edit re-prices all listings at once (pure logic in
//! [`crate::repricing`]). The review table shows each change with its reason,
//! big moves wait for a tick, and the export writes the ticked changes plus an
//! undo file. The state methods below hold the screen's logic and never touch
//! egui, so they are tested without a UI.

mod review;
mod rules;

use crate::api::cardmarket::PriceGuide;
use crate::api::inventory_sync::{InventorySyncClient, LatestPrice};
use crate::formatters::format_price_diff_csv;
use crate::inventory_db::{self, card_variant_key};
use crate::io::read_csv_with_category;
use crate::models::Card;
use crate::repricing::{
    default_ruleset_path, distinct_by_copies, evaluate, listings_from_cards, load_ruleset,
    save_ruleset, summarize, ticked_changes, Evaluation, Listing, ListingFacts, Proposal, Rule,
    Ruleset, Summary,
};
use crate::ui::state::{AppState, Screen};
use crate::ui::style;
use chrono::NaiveDate;
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// How long the ruleset must stay unchanged before it is saved (a dragged
/// number changes every frame).
const AUTOSAVE_DELAY: Duration = Duration::from_millis(800);

/// Rarities offered before a CSV is loaded.
const DEFAULT_RARITIES: [&str; 4] = ["Common", "Uncommon", "Rare", "Mythic"];
/// Languages offered before a CSV is loaded.
const DEFAULT_LANGUAGES: [&str; 2] = ["English", "German"];

/// A one-line status message.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub error: bool,
    pub text: String,
}

impl Note {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            error: false,
            text: text.into(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            error: true,
            text: text.into(),
        }
    }
}

/// What the inventory DB contributed to the listings.
#[derive(Clone, Debug, PartialEq)]
pub enum FactsStatus {
    NotLoaded,
    /// Listings the DB knew a listing date for.
    Loaded {
        known: usize,
    },
    Failed(String),
}

/// Market rows for the listings, and where they came from.
pub struct MarketData {
    pub rows: HashMap<u64, LatestPrice>,
    pub source: String,
}

type MarketResult = Result<MarketData, String>;

/// Which proposals the review table lists.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReviewFilter {
    Changes,
    Raises,
    Lowers,
    NeedsReview,
    Unchanged,
    All,
}

impl ReviewFilter {
    pub const ALL: [ReviewFilter; 6] = [
        ReviewFilter::Changes,
        ReviewFilter::Raises,
        ReviewFilter::Lowers,
        ReviewFilter::NeedsReview,
        ReviewFilter::Unchanged,
        ReviewFilter::All,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ReviewFilter::Changes => "Changes",
            ReviewFilter::Raises => "Raises",
            ReviewFilter::Lowers => "Drops",
            ReviewFilter::NeedsReview => "Needs review",
            ReviewFilter::Unchanged => "Unchanged",
            ReviewFilter::All => "All",
        }
    }

    fn accepts(self, p: &Proposal, ticked: bool) -> bool {
        use crate::repricing::Outcome;
        match self {
            ReviewFilter::Changes => p.outcome.is_change(),
            ReviewFilter::Raises => p.outcome == Outcome::Raise,
            ReviewFilter::Lowers => p.outcome == Outcome::Lower,
            ReviewFilter::NeedsReview => p.outcome.is_change() && p.review && !ticked,
            ReviewFilter::Unchanged => !p.outcome.is_change(),
            ReviewFilter::All => true,
        }
    }
}

/// Review table sort column.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReviewSort {
    /// |change| × copies.
    Impact,
    /// Change in %.
    Change,
    Name,
    Set,
    Quantity,
    Market,
    Current,
    New,
    Rule,
}

#[derive(Clone, PartialEq)]
struct RowsKey {
    eval_version: u64,
    ticks_version: u64,
    filter: ReviewFilter,
    rule_filter: Option<usize>,
    search: String,
    sort: ReviewSort,
    sort_desc: bool,
}

pub struct PricingState {
    // ── Stock ────────────────────────────────────────────────────────────
    pub(crate) csv_path: String,
    pub(crate) file_name: String,
    pub(crate) cards: Vec<Card>,
    pub(crate) category: String,
    pub(crate) listings: Vec<Listing>,
    /// CSV rows without stock (the report keeps sold-out articles).
    pub(crate) sold_out_rows: usize,
    pub(crate) load_error: Option<String>,
    /// Values offered in the rule editor's chips.
    pub(crate) rarities: Vec<String>,
    pub(crate) languages: Vec<String>,
    /// DB generation the listing facts were read at.
    facts_generation: Option<u64>,
    pub(crate) facts_status: FactsStatus,

    // ── Market ───────────────────────────────────────────────────────────
    pub(crate) market: HashMap<u64, LatestPrice>,
    pub(crate) market_source: Option<String>,
    pub(crate) market_error: Option<String>,
    market_rx: Option<Receiver<MarketResult>>,
    /// Status line while a market fetch runs.
    pub(crate) market_loading: Option<String>,

    // ── Rules ────────────────────────────────────────────────────────────
    pub(crate) ruleset: Ruleset,
    pub(crate) selected_rule: Option<usize>,
    pub(crate) ruleset_path: PathBuf,
    /// The ruleset as last written to `ruleset_path`.
    saved: Ruleset,
    dirty_since: Option<Instant>,
    pub(crate) ruleset_note: Option<Note>,
    /// The ruleset before the last open/replace, for one-click undo.
    pub(crate) undo_ruleset: Option<Ruleset>,

    // ── Evaluation ───────────────────────────────────────────────────────
    pub(crate) eval: Evaluation,
    evaluated: Option<(Ruleset, u64, NaiveDate)>,
    /// Bumped whenever listings, market rows or manual prices change.
    data_version: u64,
    eval_version: u64,
    pub(crate) summary: Summary,

    // ── Review ───────────────────────────────────────────────────────────
    pub(crate) filter: ReviewFilter,
    pub(crate) rule_filter: Option<usize>,
    pub(crate) search: String,
    pub(crate) sort: ReviewSort,
    pub(crate) sort_desc: bool,
    /// Listing indices shown in the table, filtered and sorted.
    pub(crate) rows: Vec<usize>,
    rows_key: Option<RowsKey>,
    /// Ticks the user set by hand (listing index → ticked); everything else
    /// follows [`Proposal::default_ticked`].
    pub(crate) ticks: HashMap<usize, bool>,
    ticks_version: u64,
    /// Prices typed by hand (listing index → price).
    pub(crate) manual: HashMap<usize, f64>,
    pub(crate) selected_row: Option<usize>,
    pub(crate) manual_input: f64,
    pub(crate) export_note: Option<Note>,
}

impl Default for PricingState {
    fn default() -> Self {
        Self::new(default_ruleset_path())
    }
}

impl PricingState {
    /// State whose ruleset lives at `ruleset_path` (loaded now, saved on
    /// every change). A missing file starts from the starter rules; an
    /// unreadable one is moved aside so it is never overwritten.
    pub fn new(ruleset_path: PathBuf) -> Self {
        let (ruleset, note) = if !ruleset_path.exists() {
            (Ruleset::starter(), None)
        } else {
            match load_ruleset(&ruleset_path) {
                Ok(rs) => (rs, None),
                Err(e) => {
                    let backup = ruleset_path.with_extension("broken.json");
                    let kept = std::fs::rename(&ruleset_path, &backup).is_ok();
                    log::warn!("Repricing: could not load saved rules: {e}");
                    let text = if kept {
                        format!(
                            "{e}. Kept that file as {} and started from the starter rules.",
                            backup.display()
                        )
                    } else {
                        format!("{e}. Started from the starter rules.")
                    };
                    (Ruleset::starter(), Some(Note::error(text)))
                }
            }
        };
        Self {
            csv_path: String::new(),
            file_name: String::new(),
            cards: Vec::new(),
            category: String::new(),
            listings: Vec::new(),
            sold_out_rows: 0,
            load_error: None,
            rarities: DEFAULT_RARITIES.iter().map(|s| s.to_string()).collect(),
            languages: DEFAULT_LANGUAGES.iter().map(|s| s.to_string()).collect(),
            facts_generation: None,
            facts_status: FactsStatus::NotLoaded,
            market: HashMap::new(),
            market_source: None,
            market_error: None,
            market_rx: None,
            market_loading: None,
            saved: ruleset.clone(),
            ruleset,
            selected_rule: None,
            ruleset_path,
            dirty_since: None,
            ruleset_note: note,
            undo_ruleset: None,
            eval: Evaluation::default(),
            evaluated: None,
            data_version: 0,
            eval_version: 0,
            summary: Summary::default(),
            filter: ReviewFilter::Changes,
            rule_filter: None,
            search: String::new(),
            sort: ReviewSort::Impact,
            sort_desc: true,
            rows: Vec::new(),
            rows_key: None,
            ticks: HashMap::new(),
            ticks_version: 0,
            manual: HashMap::new(),
            selected_row: None,
            manual_input: 0.0,
            export_note: None,
        }
    }

    // ── Stock ───────────────────────────────────────────────────────────────

    /// Takes a freshly loaded export. Listing facts are read from the DB on
    /// the next frame; hand-made ticks and prices belong to the old file and
    /// are dropped.
    pub(crate) fn set_source(&mut self, file_name: String, cards: Vec<Card>, category: String) {
        self.file_name = file_name;
        self.cards = cards;
        self.category = category;
        self.load_error = None;
        self.export_note = None;
        self.ticks.clear();
        self.manual.clear();
        self.selected_row = None;
        self.facts_generation = None;
        self.facts_status = FactsStatus::NotLoaded;
        self.rebuild_listings(&HashMap::new(), &HashMap::new(), NaiveDate::MIN);
    }

    /// Joins the CSV rows with the DB's listing dates and sale history
    /// (both keyed by article key). A failed read leaves the facts empty.
    pub(crate) fn apply_facts(
        &mut self,
        dates: Result<HashMap<String, String>, String>,
        sales: Result<HashMap<String, Vec<(String, i64)>>, String>,
        today: NaiveDate,
    ) {
        match (dates, sales) {
            (Ok(dates), Ok(sales)) => {
                self.rebuild_listings(&dates, &sales, today);
                let known = self
                    .listings
                    .iter()
                    .filter(|l| l.facts.age_days.is_some())
                    .count();
                self.facts_status = FactsStatus::Loaded { known };
            }
            (Err(e), _) | (_, Err(e)) => {
                self.rebuild_listings(&HashMap::new(), &HashMap::new(), today);
                self.facts_status = FactsStatus::Failed(e);
            }
        }
    }

    fn rebuild_listings(
        &mut self,
        dates: &HashMap<String, String>,
        sales: &HashMap<String, Vec<(String, i64)>>,
        today: NaiveDate,
    ) {
        let category = self.category.clone();
        self.listings = listings_from_cards(&self.cards, |card| {
            if dates.is_empty() && sales.is_empty() {
                return ListingFacts::default();
            }
            let key = card_variant_key(card, &category);
            ListingFacts::from_history(
                dates.get(&key).map(String::as_str),
                sales.get(&key).map_or(&[][..], Vec::as_slice),
                today,
            )
        });
        self.sold_out_rows = self.cards.len() - self.listings.len();
        self.rarities = merge_defaults(
            distinct_by_copies(&self.listings, |l| &l.rarity),
            &DEFAULT_RARITIES,
        );
        self.languages = merge_defaults(
            distinct_by_copies(&self.listings, |l| &l.language),
            &DEFAULT_LANGUAGES,
        );
        self.data_version += 1;
    }

    /// Distinct Cardmarket product ids of the listings.
    pub(crate) fn product_ids(&self) -> Vec<u64> {
        let ids: HashSet<u64> = self.listings.iter().filter_map(|l| l.product_id).collect();
        let mut ids: Vec<u64> = ids.into_iter().collect();
        ids.sort_unstable();
        ids
    }

    // ── Market ──────────────────────────────────────────────────────────────

    pub(crate) fn set_market(&mut self, data: MarketData) {
        self.market = data.rows;
        self.market_source = Some(data.source);
        self.market_error = None;
        self.data_version += 1;
    }

    /// Listings that have a market row.
    pub(crate) fn market_coverage(&self) -> usize {
        self.listings
            .iter()
            .filter(|l| l.product_id.is_some_and(|id| self.market.contains_key(&id)))
            .count()
    }

    // ── Evaluation ──────────────────────────────────────────────────────────

    /// Re-prices every listing when the rules, data or date changed, then
    /// refreshes the summary and the table rows.
    pub(crate) fn refresh(&mut self, today: NaiveDate) {
        let fresh = matches!(&self.evaluated,
            Some((rs, v, d)) if *v == self.data_version && *d == today && *rs == self.ruleset);
        if !fresh {
            self.eval = evaluate(
                &self.ruleset,
                &self.listings,
                &self.market,
                &self.manual,
                today,
            );
            self.evaluated = Some((self.ruleset.clone(), self.data_version, today));
            self.eval_version += 1;
            if self
                .rule_filter
                .is_some_and(|r| r >= self.ruleset.rules.len())
            {
                self.rule_filter = None;
            }
            self.recompute_summary();
        }
        self.refresh_rows();
    }

    fn recompute_summary(&mut self) {
        let ticks = &self.ticks;
        self.summary = summarize(&self.eval, &self.listings, |i, p| tick_of(ticks, i, p));
    }

    fn refresh_rows(&mut self) {
        let key = RowsKey {
            eval_version: self.eval_version,
            ticks_version: self.ticks_version,
            filter: self.filter,
            rule_filter: self.rule_filter,
            search: self.search.clone(),
            sort: self.sort,
            sort_desc: self.sort_desc,
        };
        if self.rows_key.as_ref() == Some(&key) {
            return;
        }
        self.rows = self.visible_rows();
        self.rows_key = Some(key);
    }

    /// Listing indices passing the filters, in table order.
    fn visible_rows(&self) -> Vec<usize> {
        let terms: Vec<String> = self
            .search
            .split(',')
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        let mut rows: Vec<usize> = self
            .eval
            .proposals
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                let l = &self.listings[*i];
                self.filter.accepts(p, self.is_ticked(*i))
                    && self.rule_filter.is_none_or(|r| p.rule == Some(r))
                    && (terms.is_empty()
                        || terms.iter().any(|t| {
                            l.name.to_lowercase().contains(t)
                                || l.set.to_lowercase().contains(t)
                                || l.set_code.to_lowercase() == *t
                        }))
            })
            .map(|(i, _)| i)
            .collect();
        let p = &self.eval.proposals;
        let l = &self.listings;
        let f = |a: f64, b: f64| a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
        let impact = |i: usize| {
            if p[i].outcome.is_change() {
                p[i].delta().abs() * l[i].quantity as f64
            } else {
                0.0
            }
        };
        let change = |i: usize| {
            if p[i].outcome.is_change() {
                p[i].delta_pct()
            } else {
                0.0
            }
        };
        rows.sort_by(|&a, &b| {
            let ord = match self.sort {
                ReviewSort::Impact => f(impact(a), impact(b)),
                ReviewSort::Change => f(change(a), change(b)),
                ReviewSort::Name => l[a].name.cmp(&l[b].name),
                ReviewSort::Set => l[a]
                    .set_code
                    .cmp(&l[b].set_code)
                    .then_with(|| l[a].name.cmp(&l[b].name)),
                ReviewSort::Quantity => l[a].quantity.cmp(&l[b].quantity),
                ReviewSort::Market => f(p[a].market.unwrap_or(-1.0), p[b].market.unwrap_or(-1.0)),
                ReviewSort::Current => f(p[a].old_price, p[b].old_price),
                ReviewSort::New => f(p[a].new_price, p[b].new_price),
                ReviewSort::Rule => p[a]
                    .rule
                    .unwrap_or(usize::MAX)
                    .cmp(&p[b].rule.unwrap_or(usize::MAX)),
            };
            let ord = if self.sort_desc { ord.reverse() } else { ord };
            // Stable, readable order among equals.
            ord.then_with(|| l[a].name.cmp(&l[b].name)).then(a.cmp(&b))
        });
        rows
    }

    /// Clicking a header sorts by it; clicking it again flips the direction.
    /// Numbers start largest first, text A→Z.
    pub(crate) fn sort_by(&mut self, column: ReviewSort) {
        if self.sort == column {
            self.sort_desc = !self.sort_desc;
        } else {
            self.sort = column;
            self.sort_desc = !matches!(
                column,
                ReviewSort::Name | ReviewSort::Set | ReviewSort::Rule
            );
        }
    }

    // ── Ticks & manual prices ───────────────────────────────────────────────

    pub(crate) fn is_ticked(&self, i: usize) -> bool {
        self.eval
            .proposals
            .get(i)
            .is_some_and(|p| tick_of(&self.ticks, i, p))
    }

    pub(crate) fn set_tick(&mut self, i: usize, ticked: bool) {
        self.ticks.insert(i, ticked);
        self.ticks_changed();
    }

    /// Ticks or unticks every change currently shown.
    pub(crate) fn tick_shown(&mut self, ticked: bool) {
        for &i in &self.rows {
            if self.eval.proposals[i].outcome.is_change() {
                self.ticks.insert(i, ticked);
            }
        }
        self.ticks_changed();
    }

    /// Back to the defaults: every change ticked except those needing review.
    pub(crate) fn reset_ticks(&mut self) {
        self.ticks.clear();
        self.ticks_changed();
    }

    fn ticks_changed(&mut self) {
        self.ticks_version += 1;
        self.recompute_summary();
    }

    pub(crate) fn set_manual(&mut self, i: usize, price: f64) {
        if price.is_finite() && price > 0.0 {
            self.manual.insert(i, price);
            self.ticks.remove(&i);
            self.data_version += 1;
        }
    }

    pub(crate) fn clear_manual(&mut self, i: usize) {
        if self.manual.remove(&i).is_some() {
            self.data_version += 1;
        }
    }

    // ── Rule list edits ─────────────────────────────────────────────────────

    pub(crate) fn add_rule(&mut self, rule: Rule) {
        // New rules go before a trailing catch-all, which would shadow them.
        let at = match self.ruleset.rules.last() {
            Some(last) if last.conditions.is_empty() && !rule.conditions.is_empty() => {
                self.ruleset.rules.len() - 1
            }
            _ => self.ruleset.rules.len(),
        };
        self.ruleset.rules.insert(at, rule);
        self.selected_rule = Some(at);
        self.rule_filter = None;
    }

    pub(crate) fn move_rule(&mut self, i: usize, up: bool) {
        let j = if up { i.checked_sub(1) } else { Some(i + 1) };
        let Some(j) = j.filter(|j| *j < self.ruleset.rules.len()) else {
            return;
        };
        self.ruleset.rules.swap(i, j);
        let remap = |x: Option<usize>| {
            x.map(|x| {
                if x == i {
                    j
                } else if x == j {
                    i
                } else {
                    x
                }
            })
        };
        self.selected_rule = remap(self.selected_rule);
        self.rule_filter = remap(self.rule_filter);
    }

    pub(crate) fn duplicate_rule(&mut self, i: usize) {
        let Some(rule) = self.ruleset.rules.get(i).cloned() else {
            return;
        };
        let mut copy = rule;
        copy.name = format!("{} (copy)", copy.name);
        self.ruleset.rules.insert(i + 1, copy);
        self.selected_rule = Some(i + 1);
        self.rule_filter = None;
    }

    pub(crate) fn delete_rule(&mut self, i: usize) {
        if i >= self.ruleset.rules.len() {
            return;
        }
        self.ruleset.rules.remove(i);
        let shift = |x: Option<usize>| match x {
            Some(x) if x == i => None,
            Some(x) if x > i => Some(x - 1),
            other => other,
        };
        self.selected_rule = shift(self.selected_rule);
        self.rule_filter = shift(self.rule_filter);
    }

    /// Swaps in another ruleset (opened file, starter rules), keeping the
    /// current one for undo.
    pub(crate) fn replace_ruleset(&mut self, ruleset: Ruleset, note: Note) {
        self.undo_ruleset = Some(std::mem::replace(&mut self.ruleset, ruleset));
        self.selected_rule = None;
        self.rule_filter = None;
        self.ruleset_note = Some(note);
    }

    pub(crate) fn undo_replace(&mut self) {
        if let Some(previous) = self.undo_ruleset.take() {
            self.ruleset = previous;
            self.selected_rule = None;
            self.rule_filter = None;
            self.ruleset_note = Some(Note::ok("Restored your previous rules"));
        }
    }

    /// Saves the ruleset once it has been left unchanged for
    /// [`AUTOSAVE_DELAY`]. Returns how long to wait before checking again.
    pub(crate) fn autosave(&mut self, now: Instant) -> Option<Duration> {
        if self.ruleset == self.saved {
            self.dirty_since = None;
            return None;
        }
        let since = *self.dirty_since.get_or_insert(now);
        let waited = now.saturating_duration_since(since);
        if waited < AUTOSAVE_DELAY {
            return Some(AUTOSAVE_DELAY - waited);
        }
        match save_ruleset(&self.ruleset_path, &self.ruleset) {
            Ok(()) => {
                self.saved = self.ruleset.clone();
                self.dirty_since = None;
                if self.ruleset_note.as_ref().is_some_and(|n| n.error) {
                    self.ruleset_note = None;
                }
                None
            }
            Err(e) => {
                self.ruleset_note = Some(Note::error(format!(
                    "Could not save the rules to {}: {e}",
                    self.ruleset_path.display()
                )));
                // Keep the edits in memory; retry later.
                self.dirty_since = Some(now);
                Some(AUTOSAVE_DELAY * 10)
            }
        }
    }

    // ── Export ──────────────────────────────────────────────────────────────

    /// Writes the ticked changes to `path` (new prices) and to an undo file
    /// next to it (old prices). Both have the inventory export's columns.
    pub(crate) fn export_to(&self, path: &Path) -> Result<ExportReport, String> {
        let changes = ticked_changes(&self.eval, &self.listings, |i, p| {
            tick_of(&self.ticks, i, p)
        });
        if changes.is_empty() {
            return Err("Nothing is ticked for export".to_string());
        }
        let undo = undo_path(path);
        let rows: Vec<usize> = changes.iter().map(|c| c.row).collect();
        let new: HashMap<usize, f64> = changes.iter().map(|c| (c.row, c.new_price)).collect();
        let old: HashMap<usize, f64> = changes.iter().map(|c| (c.row, c.old_price)).collect();
        std::fs::write(path, format_price_diff_csv(&self.cards, &rows, &new))
            .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        std::fs::write(&undo, format_price_diff_csv(&self.cards, &rows, &old))
            .map_err(|e| format!("Could not write {}: {e}", undo.display()))?;
        Ok(ExportReport {
            changes: changes.len(),
            csv: path.to_path_buf(),
            undo,
        })
    }
}

/// What an export wrote.
#[derive(Debug, PartialEq)]
pub struct ExportReport {
    pub changes: usize,
    pub csv: PathBuf,
    pub undo: PathBuf,
}

fn tick_of(ticks: &HashMap<usize, bool>, i: usize, p: &Proposal) -> bool {
    p.outcome.is_change() && ticks.get(&i).copied().unwrap_or_else(|| p.default_ticked())
}

/// `prices.csv` → `prices_undo.csv`, in the same folder.
pub(crate) fn undo_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repricing".to_string());
    path.with_file_name(format!("{stem}_undo.csv"))
}

/// File name offered when exporting: `repricing_<date>.csv`.
pub(crate) fn export_file_name(today: NaiveDate) -> String {
    format!("repricing_{}.csv", today.format("%Y-%m-%d"))
}

/// Values found in the stock first, then defaults not seen there.
fn merge_defaults(mut found: Vec<String>, defaults: &[&str]) -> Vec<String> {
    for d in defaults {
        if !found.iter().any(|f| f.eq_ignore_ascii_case(d)) {
            found.push(d.to_string());
        }
    }
    found
}

/// Price-guide rows for `ids`, in the tracker's row format (the guide's
/// creation date becomes every row's price date).
pub(crate) fn rows_from_guide(guide: &PriceGuide, ids: &[u64]) -> HashMap<u64, LatestPrice> {
    let date: String = guide.created_at().chars().take(10).collect();
    ids.iter()
        .filter_map(|id| guide.get(*id))
        .map(|e| {
            (
                e.id_product,
                LatestPrice {
                    id_product: e.id_product,
                    price_date: date.clone(),
                    avg: e.avg,
                    low: e.low,
                    trend: e.trend,
                    avg1: e.avg1,
                    avg7: e.avg7,
                    avg30: e.avg30,
                    avg_foil: e.avg_foil,
                    low_foil: e.low_foil,
                    trend_foil: e.trend_foil,
                    avg1_foil: e.avg1_foil,
                    avg7_foil: e.avg7_foil,
                    avg30_foil: e.avg30_foil,
                },
            )
        })
        .collect()
}

/// Newest price date among the rows (`YYYY-MM-DD`).
pub(crate) fn newest_date(rows: &HashMap<u64, LatestPrice>) -> Option<String> {
    rows.values()
        .map(|r| r.price_date.chars().take(10).collect::<String>())
        .max()
}

// ── Screen ──────────────────────────────────────────────────────────────────

pub struct PricingScreen;

impl PricingScreen {
    pub fn show(ctx: &egui::Context, app_state: &mut AppState, state: &mut PricingState) {
        let today = chrono::Local::now().date_naive();
        Self::poll_market(state);
        Self::sync_with_db(app_state, state, today);
        if let Some(wait) = state.autosave(Instant::now()) {
            ctx.request_repaint_after(wait);
        }
        if state.market_loading.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        Self::show_header(ctx, app_state, state);
        state.refresh(today);
        let before = state.eval_version;
        rules::show(ctx, state);
        state.refresh(today);
        if state.eval_version != before {
            // A rule edit re-priced everything; redraw so the rule list's
            // counters catch up with the table.
            ctx.request_repaint();
        }
        review::show(ctx, state, today);
    }

    fn show_header(ctx: &egui::Context, app_state: &mut AppState, state: &mut PricingState) {
        egui::TopBottomPanel::top("pricing_header")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ctx, |ui| {
                if style::back_button(ui, "Back") {
                    app_state.current_screen = Screen::Welcome;
                }
                ui.add_space(4.0);
                style::screen_heading(ui, "Repricing");
                ui.columns(2, |cols| {
                    Self::show_stock_step(&mut cols[0], app_state, state);
                    Self::show_market_step(&mut cols[1], ctx, app_state, state);
                });
            });
    }

    fn show_stock_step(ui: &mut egui::Ui, app_state: &mut AppState, state: &mut PricingState) {
        style::section_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            step_title(ui, "1", "Your stock");
            ui.horizontal(|ui| {
                if style::secondary_button(ui, "Load inventory CSV…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("CSV", &["csv"])
                        .pick_file()
                    {
                        state.csv_path = path.display().to_string();
                        Self::load_csv(app_state, state);
                    }
                }
                if !state.cards.is_empty() && style::secondary_button(ui, "Reload").clicked() {
                    Self::load_csv(app_state, state);
                }
                if !state.file_name.is_empty() {
                    ui.label(egui::RichText::new(&state.file_name).strong());
                }
            });
            if let Some(err) = &state.load_error {
                style::status_error(ui, err);
            } else if state.cards.is_empty() {
                muted(
                    ui,
                    "The Cardmarket inventory export (Magic singles). It is also synced into the \
                     inventory database, like on the other screens.",
                );
            } else {
                let copies: i64 = state.listings.iter().map(|l| l.quantity).sum();
                let value: f64 = state
                    .listings
                    .iter()
                    .map(|l| l.price * l.quantity as f64)
                    .sum();
                ui.label(format!(
                    "{} listings in stock · {} copies · {} listed",
                    thousands(state.listings.len() as i64),
                    thousands(copies),
                    crate::repricing::euro(value)
                ));
                let mut extra = Vec::new();
                if state.sold_out_rows > 0 {
                    extra.push(format!(
                        "{} sold-out rows ignored",
                        thousands(state.sold_out_rows as i64)
                    ));
                }
                match &state.facts_status {
                    FactsStatus::NotLoaded => {}
                    FactsStatus::Loaded { known } => extra.push(format!(
                        "age and sales known for {} of {}",
                        thousands(*known as i64),
                        thousands(state.listings.len() as i64)
                    )),
                    FactsStatus::Failed(e) => extra.push(format!(
                        "inventory DB unavailable ({e}): age and sales conditions match nothing"
                    )),
                }
                if !extra.is_empty() {
                    muted(ui, &extra.join(" · "));
                }
            }
        });
    }

    fn show_market_step(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        app_state: &mut AppState,
        state: &mut PricingState,
    ) {
        style::section_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            step_title(ui, "2", "Market prices");
            let url = app_state.inventory_sync_url.clone();
            let has_stock = !state.listings.is_empty();
            let busy = state.market_loading.is_some();
            crate::ui::components::InventorySyncBar::show(ui, ctx, app_state, |ui, connected| {
                let enabled = connected && has_stock && !busy;
                if style::primary_button_enabled(ui, "Fetch prices", enabled)
                    .on_hover_text(
                        "Latest prices for every listing from the inventory_sync tracker",
                    )
                    .clicked()
                {
                    Self::spawn_tracker_fetch(state, &url);
                }
            });
            ui.horizontal(|ui| {
                muted(ui, "or the Cardmarket price guide:");
                if style::secondary_button_enabled(ui, "Download", has_stock && !busy)
                    .on_hover_text("Downloads Cardmarket's full price guide (~50 MB)")
                    .clicked()
                {
                    Self::spawn_guide_fetch(state, None);
                }
                if style::secondary_button_enabled(ui, "From file…", has_stock && !busy).clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .pick_file()
                    {
                        Self::spawn_guide_fetch(state, Some(path));
                    }
                }
            });
            if let Some(msg) = &state.market_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    muted(ui, msg);
                });
            } else if let Some(err) = &state.market_error {
                style::status_error(ui, err);
            } else if let Some(source) = &state.market_source {
                ui.label(format!(
                    "{source} · {} of {} listings covered",
                    thousands(state.market_coverage() as i64),
                    thousands(state.listings.len() as i64)
                ));
            } else if has_stock {
                muted(
                    ui,
                    "Rules based on market prices need these. Without them only current-price \
                     and fixed-price rules apply.",
                );
            }
        });
    }

    fn load_csv(app_state: &mut AppState, state: &mut PricingState) {
        let path = state.csv_path.trim().to_string();
        if path.is_empty() {
            return;
        }
        let file_name = Path::new(&path)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        match read_csv_with_category(&path) {
            Ok(csv) => {
                log::info!(
                    "Repricing: loaded {} rows from {file_name} (category '{}')",
                    csv.cards.len(),
                    csv.category
                );
                // Same as every screen that loads an export: keep the DB current.
                app_state.sync_inventory_guarded(&csv.cards, &csv.category);
                state.set_source(file_name, csv.cards, csv.category);
            }
            Err(e) => state.load_error = Some(format!("Could not read {file_name}: {e}")),
        }
    }

    /// Reads listing dates and sales whenever the stock or the DB changed.
    fn sync_with_db(app_state: &AppState, state: &mut PricingState, today: NaiveDate) {
        if state.cards.is_empty() || state.facts_generation == Some(app_state.db_generation) {
            return;
        }
        state.facts_generation = Some(app_state.db_generation);
        let category = state.category.clone();
        state.apply_facts(
            inventory_db::get_listing_dates(&category).map_err(|e| e.to_string()),
            inventory_db::get_sales_by_variant(&category).map_err(|e| e.to_string()),
            today,
        );
    }

    fn poll_market(state: &mut PricingState) {
        let Some(rx) = &state.market_rx else { return };
        match rx.try_recv() {
            Ok(result) => {
                state.market_rx = None;
                state.market_loading = None;
                match result {
                    Ok(data) => {
                        log::info!("Repricing: {}", data.source);
                        state.set_market(data);
                    }
                    Err(e) => {
                        log::error!("Repricing: market fetch failed: {e}");
                        state.market_error = Some(e);
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                state.market_rx = None;
                state.market_loading = None;
                state.market_error = Some("The price fetch stopped unexpectedly".to_string());
            }
        }
    }

    fn spawn_tracker_fetch(state: &mut PricingState, url: &str) {
        let ids = state.product_ids();
        if ids.is_empty() {
            state.market_error = Some("No listing has a Cardmarket product id".to_string());
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        state.market_rx = Some(rx);
        state.market_error = None;
        state.market_loading = Some(format!(
            "Fetching prices for {} products…",
            thousands(ids.len() as i64)
        ));
        let client = InventorySyncClient::new(url);
        std::thread::spawn(move || {
            let result = client
                .latest_prices_blocking(&ids)
                .map(|prices| {
                    let rows: HashMap<u64, LatestPrice> =
                        prices.into_iter().map(|p| (p.id_product, p)).collect();
                    let source = format!(
                        "Tracker prices{}",
                        newest_date(&rows)
                            .map(|d| format!(" from {d}"))
                            .unwrap_or_default()
                    );
                    MarketData { rows, source }
                })
                .map_err(|e| format!("Price fetch failed: {e}"));
            let _ = tx.send(result);
        });
    }

    /// Downloads the price guide, or reads it from `file`, off the UI thread.
    fn spawn_guide_fetch(state: &mut PricingState, file: Option<PathBuf>) {
        let ids = state.product_ids();
        let (tx, rx) = std::sync::mpsc::channel();
        state.market_rx = Some(rx);
        state.market_error = None;
        state.market_loading = Some(if file.is_some() {
            "Reading the price guide…".to_string()
        } else {
            "Downloading the Cardmarket price guide (~50 MB)…".to_string()
        });
        std::thread::spawn(move || {
            let guide = match &file {
                Some(path) => PriceGuide::load(&path.to_string_lossy()),
                None => PriceGuide::fetch_blocking(),
            };
            let result = guide
                .map(|g| {
                    let rows = rows_from_guide(&g, &ids);
                    let date: String = g.created_at().chars().take(10).collect();
                    MarketData {
                        rows,
                        source: format!("Cardmarket price guide from {date}"),
                    }
                })
                .map_err(|e| format!("Price guide failed: {e}"));
            let _ = tx.send(result);
        });
    }
}

// ── Small shared widgets ────────────────────────────────────────────────────

fn step_title(ui: &mut egui::Ui, number: &str, title: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(number)
                .strong()
                .color(style::ACCENT)
                .size(15.0),
        );
        ui.label(
            egui::RichText::new(title)
                .strong()
                .size(14.0)
                .color(style::TEXT_PRIMARY),
        );
    });
}

pub(super) fn muted(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(
        egui::RichText::new(text)
            .size(11.5)
            .color(style::TEXT_MUTED),
    )
}

/// `12345` → `12,345`.
pub(crate) fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
