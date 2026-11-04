//! Bulk Lots screen: bundle old, cheap singles into one Cardmarket lot
//! ("100 commons", "100 non-basic lands").
//!
//! The "New lot" tab walks through three steps — choose the cards (rules and
//! a live preview), pick them from the shelves (image tiles), finish (TCG
//! Powertools CSV, then the lot is recorded) — and the "History" tab lets the
//! user correct a lot's price, confirm its removal or undo it.
//!
//! Selection rules live in [`crate::bulk_lot`]; revenue and the hand-off to
//! the next inventory sync in `inventory_db::bulk_lots`. The state methods
//! below hold the screen's logic and never touch egui, so they are tested
//! without a UI.

mod define;
mod finish;
mod history;
mod pick;
mod widgets;

use crate::bulk_lot::{
    build_candidates, build_lot, default_lot_name, funnel, sort_for_picking, summarize, top_up,
    type_lookups_needed, Candidate, CardType, Funnel, LotPick, LotSpec, LotSummary,
};
use crate::cache::CardTypeCache;
use crate::card_types::{lookup_types, LookupOutcome, LookupProgress, Printing};
use crate::category::DEFAULT_CATEGORY;
use crate::inventory_db::{self, BulkLotCard, BulkLotSummary, PendingRemoval};
use crate::io::read_csv_with_category;
use crate::models::Card;
use crate::ui::components::CardImages;
use crate::ui::state::{AppState, Screen};
use crate::ui::style;
use chrono::NaiveDate;
use eframe::egui;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;

/// Days after which copies still awaiting a sync are flagged in the history.
pub(crate) const STALE_PENDING_DAYS: i64 = 7;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    NewLot,
    History,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Define,
    Pick,
    Done,
}

/// The inventory CSV the lot is built from.
pub struct LoadedCsv {
    pub file_name: String,
    pub cards: Vec<Card>,
    pub category: String,
}

impl LoadedCsv {
    fn is_magic(&self) -> bool {
        self.category == DEFAULT_CATEGORY
    }
}

/// One tile of the picking step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickLine {
    pub pick: LotPick,
    pub picked: bool,
}

enum TypeMsg {
    Progress(LookupProgress),
    Done(LookupOutcome),
}

/// A running Scryfall type lookup.
pub struct TypeJob {
    rx: Receiver<TypeMsg>,
    cancel: Arc<AtomicBool>,
    pub progress: Option<LookupProgress>,
    pub printings: usize,
}

/// The open "Finish lot" dialog.
pub struct FinishDialog {
    pub name: String,
    pub price: f64,
    pub error: Option<String>,
    /// Where the CSV went, once saved — a failed recording is retried
    /// without saving the file again.
    pub saved_csv: Option<PathBuf>,
}

/// What the "Done" step reports.
pub struct DoneInfo {
    pub name: String,
    pub copies: i64,
    pub price: f64,
    pub csv: PathBuf,
}

/// A question the user must answer before something irreversible happens.
pub enum Confirm {
    /// Leave the picking step and drop the ticks.
    LeavePicking,
    UndoLot(BulkLotSummary),
    MarkRemoved(BulkLotSummary),
}

pub struct BulkLotState {
    pub tab: Tab,
    pub step: Step,

    // ── Source ──────────────────────────────────────────────────────────────
    pub csv_path: String,
    pub source: Option<LoadedCsv>,
    pub load_error: Option<String>,
    /// DB generation the candidates were built at (`None` = rebuild).
    candidates_generation: Option<u64>,
    pub candidates: Vec<Candidate>,
    /// Languages in stock, most copies first.
    pub languages: Vec<(String, i64)>,

    // ── Definition ──────────────────────────────────────────────────────────
    pub spec: LotSpec,
    pub name: String,
    /// The user typed a name; stop suggesting one.
    pub name_touched: bool,
    pub price: f64,
    /// Spec the preview was computed for (`None` = recompute).
    preview_spec: Option<LotSpec>,
    pub preview: Vec<LotPick>,
    pub funnel: Funnel,
    /// Candidates whose type line still has to be looked up.
    lookups: Vec<usize>,
    /// Card type of the last preview, to notice when the user picks another.
    last_card_type: Option<CardType>,

    // ── Card types ──────────────────────────────────────────────────────────
    pub types: CardTypeCache,
    pub type_job: Option<TypeJob>,
    /// No automatic lookup until the user resumes (after cancel or error).
    pub type_paused: bool,
    pub type_error: Option<String>,

    // ── Picking ─────────────────────────────────────────────────────────────
    pub lines: Vec<PickLine>,
    /// Candidates the user replaced; never suggested again for this lot.
    excluded: HashSet<usize>,
    pub hide_picked: bool,
    pub images: CardImages,
    pub pick_notice: Option<String>,
    /// Candidate swapped in last, highlighted until the given time.
    pub flash: Option<(usize, f64)>,

    // ── Finish ──────────────────────────────────────────────────────────────
    pub finish: Option<FinishDialog>,
    pub done: Option<DoneInfo>,
    pub confirm: Option<Confirm>,

    // ── History ─────────────────────────────────────────────────────────────
    pub history: Vec<BulkLotSummary>,
    history_generation: Option<u64>,
    pub history_error: Option<String>,
    /// Lot whose cards are listed under the table.
    pub selected_lot: Option<(i64, Result<Vec<BulkLotCard>, String>)>,
    /// Lot whose price is being edited, with the value in the field.
    pub price_edit: Option<(i64, f64)>,
}

impl Default for BulkLotState {
    fn default() -> Self {
        Self::with_type_cache(CardTypeCache::new())
    }
}

impl BulkLotState {
    pub fn with_type_cache(types: CardTypeCache) -> Self {
        let spec = LotSpec::default();
        Self {
            tab: Tab::NewLot,
            step: Step::Define,
            csv_path: String::new(),
            source: None,
            load_error: None,
            candidates_generation: None,
            candidates: Vec::new(),
            languages: Vec::new(),
            name: default_lot_name(&spec),
            name_touched: false,
            price: 1.0,
            spec,
            preview_spec: None,
            preview: Vec::new(),
            funnel: Funnel::default(),
            lookups: Vec::new(),
            last_card_type: None,
            types,
            type_job: None,
            type_paused: false,
            type_error: None,
            lines: Vec::new(),
            excluded: HashSet::new(),
            hide_picked: false,
            images: CardImages::new(),
            pick_notice: None,
            flash: None,
            finish: None,
            done: None,
            confirm: None,
            history: Vec::new(),
            history_generation: None,
            history_error: None,
            selected_lot: None,
            price_edit: None,
        }
    }

    // ── Source ──────────────────────────────────────────────────────────────

    /// A freshly loaded CSV; resets everything built from the previous one.
    pub(crate) fn set_source(&mut self, file_name: String, cards: Vec<Card>, category: String) {
        self.source = Some(LoadedCsv {
            file_name,
            cards,
            category,
        });
        self.load_error = None;
        self.candidates.clear();
        self.candidates_generation = None;
        self.preview_spec = None;
        self.lines.clear();
        self.excluded.clear();
        self.step = Step::Define;
    }

    /// Whether a Magic export is loaded (lots are built from cards only).
    pub(crate) fn has_magic_source(&self) -> bool {
        self.source.as_ref().is_some_and(LoadedCsv::is_magic)
    }

    /// Rebuilds the candidates from the loaded CSV with the database's
    /// listing dates and the copies promised to lots awaiting a sync.
    pub(crate) fn rebuild_candidates(
        &mut self,
        listing_dates: &HashMap<String, String>,
        pending: &[PendingRemoval],
        today: NaiveDate,
    ) {
        let Some(source) = &self.source else { return };
        self.candidates = build_candidates(
            &source.cards,
            &source.category,
            listing_dates,
            pending,
            today,
        );
        self.fill_types();
        let mut languages: BTreeMap<&str, i64> = BTreeMap::new();
        for c in &self.candidates {
            *languages.entry(c.language.as_str()).or_insert(0) += c.available + c.reserved;
        }
        let mut languages: Vec<(String, i64)> = languages
            .into_iter()
            .map(|(l, n)| (l.to_string(), n))
            .collect();
        languages.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        self.languages = languages;
        self.preview_spec = None;
    }

    fn fill_types(&mut self) {
        for c in &mut self.candidates {
            c.front_type = self.types.type_line(&c.set_code, &c.cn).map(str::to_string);
        }
    }

    // ── Definition ──────────────────────────────────────────────────────────

    /// Recomputes funnel, preview and pending type lookups when the spec or
    /// the candidates changed. Returns whether anything was recomputed.
    pub(crate) fn refresh_preview(&mut self, today: NaiveDate) -> bool {
        if self.preview_spec.as_ref() == Some(&self.spec) {
            return false;
        }
        if self
            .last_card_type
            .is_some_and(|old| old != self.spec.card_type)
        {
            // A different type is a fresh request: look it up again.
            self.type_paused = false;
            self.type_error = None;
        }
        self.last_card_type = Some(self.spec.card_type);
        if !self.name_touched {
            self.name = default_lot_name(&self.spec);
        }
        self.funnel = funnel(&self.candidates, &self.spec);
        self.preview = build_lot(&self.candidates, &self.spec);
        sort_for_picking(&self.candidates, &mut self.preview);
        let types = &self.types;
        self.lookups = type_lookups_needed(&self.candidates, &self.spec)
            .into_iter()
            .filter(|&i| {
                let c = &self.candidates[i];
                !types.is_settled(&c.set_code, &c.cn, today)
            })
            .collect();
        self.preview_spec = Some(self.spec.clone());
        true
    }

    /// Figures of the previewed lot.
    pub(crate) fn preview_summary(&self) -> LotSummary {
        summarize(&self.candidates, &self.preview, self.spec.hit_min_price)
    }

    pub(crate) fn preview_copies(&self) -> i64 {
        self.preview.iter().map(|p| p.copies).sum()
    }

    /// Printings to send to Scryfall, with their Cardmarket ids.
    fn lookup_printings(&self) -> Vec<Printing> {
        let cards = self
            .source
            .as_ref()
            .map(|s| s.cards.as_slice())
            .unwrap_or(&[]);
        self.lookups
            .iter()
            .map(|&i| {
                let c = &self.candidates[i];
                Printing {
                    set_code: c.set_code.clone(),
                    cn: c.cn.clone(),
                    cardmarket_id: cards
                        .get(c.source)
                        .and_then(|card| card.cardmarket_id.trim().parse().ok()),
                }
            })
            .collect()
    }

    /// Whether type lines are still missing for the current filters.
    pub(crate) fn types_incomplete(&self) -> bool {
        !self.lookups.is_empty()
    }

    /// Stores a finished lookup and refreshes the preview with it.
    pub(crate) fn apply_type_outcome(&mut self, outcome: LookupOutcome, today: NaiveDate) {
        for (set, cn, type_line) in &outcome.found {
            self.types.insert(set, cn, type_line);
        }
        for (set, cn) in &outcome.missing {
            self.types.insert_missing(set, cn, today);
        }
        self.types.save();
        if let Some(e) = outcome.error {
            self.type_error = Some(e);
            self.type_paused = true;
        } else if outcome.cancelled {
            self.type_paused = true;
        }
        self.fill_types();
        self.preview_spec = None;
    }

    /// Lets the type lookup run (again) after a cancel or an error.
    pub(crate) fn resume_types(&mut self) {
        self.type_paused = false;
        self.type_error = None;
    }

    // ── Picking ─────────────────────────────────────────────────────────────

    /// Turns the preview into the picking list.
    pub(crate) fn start_picking(&mut self) {
        self.lines = self
            .preview
            .iter()
            .map(|&pick| PickLine {
                pick,
                picked: false,
            })
            .collect();
        self.excluded.clear();
        self.pick_notice = None;
        self.flash = None;
        self.step = Step::Pick;
    }

    pub(crate) fn toggle_picked(&mut self, line: usize) {
        if let Some(l) = self.lines.get_mut(line) {
            l.picked = !l.picked;
        }
    }

    pub(crate) fn set_all_picked(&mut self, picked: bool) {
        for l in &mut self.lines {
            l.picked = picked;
        }
    }

    /// Swaps a card the user cannot find (or does not want) for the next
    /// match, in place. Returns the candidates that came in.
    pub(crate) fn replace(&mut self, line: usize) -> Vec<usize> {
        if line >= self.lines.len() {
            return Vec::new();
        }
        let removed = self.lines.remove(line);
        self.excluded.insert(removed.pick.candidate);
        let remaining: Vec<LotPick> = self.lines.iter().map(|l| l.pick).collect();
        let mut added = top_up(&self.candidates, &self.spec, &remaining, &self.excluded);
        sort_for_picking(&self.candidates, &mut added);
        let name = &self.candidates[removed.pick.candidate].name;
        self.pick_notice = Some(if added.is_empty() {
            format!(
                "{name} removed — no other card matches the rules, so the lot is now {}.",
                widgets::cards(self.planned_copies())
            )
        } else {
            let incoming: Vec<&str> = added
                .iter()
                .map(|p| self.candidates[p.candidate].name.as_str())
                .collect();
            format!("{name} replaced by {}.", incoming.join(", "))
        });
        let incoming: Vec<usize> = added.iter().map(|p| p.candidate).collect();
        for (offset, pick) in added.into_iter().enumerate() {
            self.lines.insert(
                line + offset,
                PickLine {
                    pick,
                    picked: false,
                },
            );
        }
        incoming
    }

    /// Copies on the picking list.
    pub(crate) fn planned_copies(&self) -> i64 {
        self.lines.iter().map(|l| l.pick.copies).sum()
    }

    /// Copies ticked as picked.
    pub(crate) fn picked_copies(&self) -> i64 {
        self.lines
            .iter()
            .filter(|l| l.picked)
            .map(|l| l.pick.copies)
            .sum()
    }

    pub(crate) fn picked_summary(&self) -> LotSummary {
        let picks: Vec<LotPick> = self
            .lines
            .iter()
            .filter(|l| l.picked)
            .map(|l| l.pick)
            .collect();
        summarize(&self.candidates, &picks, self.spec.hit_min_price)
    }

    /// The picked copies as `(CSV row, copies)` — what the CSV and the
    /// database record.
    pub(crate) fn picked_lines(&self) -> Vec<(Card, i64)> {
        let Some(source) = &self.source else {
            return Vec::new();
        };
        self.lines
            .iter()
            .filter(|l| l.picked)
            .filter_map(|l| {
                let c = self.candidates.get(l.pick.candidate)?;
                Some((source.cards.get(c.source)?.clone(), l.pick.copies))
            })
            .collect()
    }

    /// Back to the rules; the picking list is dropped.
    pub(crate) fn back_to_define(&mut self) {
        self.lines.clear();
        self.excluded.clear();
        self.pick_notice = None;
        self.step = Step::Define;
    }

    /// The lot was recorded: show the result.
    pub(crate) fn finish_recorded(&mut self, done: DoneInfo) {
        self.price = done.price;
        self.done = Some(done);
        self.finish = None;
        self.lines.clear();
        self.excluded.clear();
        self.step = Step::Done;
    }

    /// Start over with the same rules (the recorded copies are now reserved).
    pub(crate) fn build_another(&mut self) {
        self.done = None;
        self.name_touched = false;
        self.preview_spec = None;
        self.step = Step::Define;
    }

    // ── History ─────────────────────────────────────────────────────────────

    /// Copies of all lots still awaiting a sync.
    pub(crate) fn pending_copies(&self) -> i64 {
        self.history.iter().map(|l| l.pending_copies).sum()
    }
}

pub struct BulkLotScreen;

impl BulkLotScreen {
    pub fn show(ctx: &egui::Context, app_state: &mut AppState, state: &mut BulkLotState) {
        let today = chrono::Local::now().date_naive();
        Self::poll_type_job(ctx, state, today);
        state.images.poll(ctx);
        Self::sync_with_db(app_state, state, today);
        if state.tab == Tab::NewLot && state.step == Step::Define && state.has_magic_source() {
            state.refresh_preview(today);
            Self::maybe_start_type_lookup(ctx, state);
        }

        Self::show_header(ctx, app_state, state);
        match (state.tab, state.step) {
            (Tab::History, _) => history::show(ctx, app_state, state, today),
            (Tab::NewLot, Step::Define) => define::show(ctx, app_state, state),
            (Tab::NewLot, Step::Pick) => pick::show(ctx, state),
            (Tab::NewLot, Step::Done) => finish::show_done(ctx, state),
        }
        finish::show_dialog(ctx, app_state, state, today);
        Self::show_confirm(ctx, app_state, state);
    }

    fn show_header(ctx: &egui::Context, app_state: &mut AppState, state: &mut BulkLotState) {
        egui::TopBottomPanel::top("bulk_lot_header")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ctx, |ui| {
                if style::back_button(ui, "Back") {
                    app_state.current_screen = Screen::Welcome;
                }
                ui.add_space(4.0);
                style::screen_heading(ui, "Bulk Lots");
                ui.horizontal(|ui| {
                    let history_label = if state.history.is_empty() {
                        "History".to_string()
                    } else {
                        format!("History ({})", state.history.len())
                    };
                    widgets::tabs(
                        ui,
                        &mut state.tab,
                        &[
                            (Tab::NewLot, "New lot".to_string()),
                            (Tab::History, history_label),
                        ],
                    );
                    if state.tab == Tab::NewLot {
                        ui.add_space(24.0);
                        let current = match state.step {
                            Step::Define => 0,
                            Step::Pick => 1,
                            Step::Done => 3,
                        };
                        widgets::stepper(ui, &["Choose cards", "Pick", "Finish"], current);
                    }
                });
                ui.add_space(2.0);
            });
    }

    // ── Data ────────────────────────────────────────────────────────────────

    /// Reloads what depends on the database after it changed (a sync from
    /// any screen, a recorded or edited lot).
    fn sync_with_db(app_state: &AppState, state: &mut BulkLotState, today: NaiveDate) {
        let generation = app_state.db_generation;
        if state.history_generation != Some(generation) {
            state.history_generation = Some(generation);
            match inventory_db::list_bulk_lots() {
                Ok(lots) => {
                    state.history = lots;
                    state.history_error = None;
                }
                Err(e) => {
                    state.history_error = Some(format!("Could not read the lot history: {e}"))
                }
            }
            if let Some((id, _)) = state.selected_lot {
                state.selected_lot = Some((
                    id,
                    inventory_db::get_bulk_lot_cards(id).map_err(|e| e.to_string()),
                ));
            }
        }

        // Candidates only change on the rules step: a rebuild while picking
        // would shift the list under the user's feet.
        if state.step == Step::Define
            && state.has_magic_source()
            && state.candidates_generation != Some(generation)
        {
            state.candidates_generation = Some(generation);
            let dates = inventory_db::get_listing_dates(DEFAULT_CATEGORY);
            let pending = inventory_db::get_pending_bulk_removals(DEFAULT_CATEGORY);
            match (dates, pending) {
                (Ok(dates), Ok(pending)) => state.rebuild_candidates(&dates, &pending, today),
                (Err(e), _) | (_, Err(e)) => {
                    // Without the pending removals, copies already promised
                    // to a lot could be picked twice — build nothing.
                    state.candidates.clear();
                    state.preview_spec = None;
                    state.load_error = Some(format!("Could not read the inventory database: {e}"));
                }
            }
        }
    }

    pub(super) fn load_csv(app_state: &mut AppState, state: &mut BulkLotState) {
        let path = state.csv_path.trim().to_string();
        if path.is_empty() {
            return;
        }
        let file_name = std::path::Path::new(&path)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        match read_csv_with_category(&path) {
            Ok(csv) => {
                log::info!(
                    "Bulk lots: loaded {} rows from {file_name} (category '{}')",
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

    fn maybe_start_type_lookup(ctx: &egui::Context, state: &mut BulkLotState) {
        if state.type_job.is_some() || state.type_paused || state.lookups.is_empty() {
            return;
        }
        let printings = state.lookup_printings();
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let repaint = ctx.clone();
        let count = printings.len();
        log::info!("Bulk lots: looking up {count} card types on Scryfall");
        std::thread::spawn(move || {
            let outcome = lookup_types(&printings, &worker_cancel, |p| {
                let _ = tx.send(TypeMsg::Progress(p));
                repaint.request_repaint();
            });
            let _ = tx.send(TypeMsg::Done(outcome));
            repaint.request_repaint();
        });
        state.type_job = Some(TypeJob {
            rx,
            cancel,
            progress: None,
            printings: count,
        });
    }

    fn poll_type_job(ctx: &egui::Context, state: &mut BulkLotState, today: NaiveDate) {
        let Some(job) = &mut state.type_job else {
            return;
        };
        let mut finished = None;
        loop {
            match job.rx.try_recv() {
                Ok(TypeMsg::Progress(p)) => job.progress = Some(p),
                Ok(TypeMsg::Done(outcome)) => {
                    finished = Some(outcome);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finished = Some(LookupOutcome {
                        error: Some("The card type lookup stopped unexpectedly.".to_string()),
                        ..LookupOutcome::default()
                    });
                    break;
                }
            }
        }
        match finished {
            Some(outcome) => {
                state.type_job = None;
                state.apply_type_outcome(outcome, today);
            }
            None => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
        }
    }

    pub(super) fn cancel_type_lookup(state: &mut BulkLotState) {
        if let Some(job) = &state.type_job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    // ── Confirmations ───────────────────────────────────────────────────────

    fn show_confirm(ctx: &egui::Context, app_state: &mut AppState, state: &mut BulkLotState) {
        let Some(confirm) = &state.confirm else {
            return;
        };
        let (title, body, action, danger) = match confirm {
            Confirm::LeavePicking => (
                "Leave the picking list?".to_string(),
                format!(
                    "You have ticked {} as picked. Going back to the rules drops the picking \
                     list; put the cards back on the shelf or keep them aside.",
                    widgets::cards(state.picked_copies())
                ),
                "Leave picking",
                false,
            ),
            Confirm::UndoLot(lot) => (
                format!("Undo “{}”?", lot.name),
                if lot.pending_copies > 0 {
                    format!(
                        "The lot and its {} revenue are deleted. Its {} still listed on \
                         Cardmarket count as normal stock again. Only undo if you did NOT import \
                         the CSV into TCG Powertools — otherwise the next inventory import \
                         counts them as single sales.",
                        widgets::euro(lot.price),
                        widgets::cards(lot.pending_copies)
                    )
                } else {
                    format!(
                        "The lot and its {} revenue are deleted. Its cards already left your \
                         Cardmarket stock; if you put them back on sale, the next import \
                         adds them as new stock.",
                        widgets::euro(lot.price)
                    )
                },
                "Undo lot",
                true,
            ),
            Confirm::MarkRemoved(lot) => (
                format!("Mark “{}” as removed?", lot.name),
                format!(
                    "Use this when the {} are already gone from your Cardmarket stock but no \
                     inventory import has noticed — for example because the same card was \
                     restocked at the same time. Afterwards, any drop of these cards counts \
                     as a normal sale.",
                    widgets::cards(lot.pending_copies)
                ),
                "Mark removed",
                false,
            ),
        };

        let mut decision: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::new("bulk_lot_confirm")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(
                egui::RichText::new(title)
                    .size(17.0)
                    .strong()
                    .color(style::TEXT_PRIMARY),
            );
            ui.add_space(8.0);
            ui.label(egui::RichText::new(body).color(style::TEXT_MUTED));
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let confirmed = if danger {
                    style::danger_button(ui, action)
                } else {
                    style::primary_button(ui, action)
                };
                if confirmed.clicked() {
                    decision = Some(true);
                }
                if style::secondary_button(ui, "Cancel").clicked() {
                    decision = Some(false);
                }
            });
        });
        if decision.is_none() && modal.should_close() {
            decision = Some(false);
        }
        let Some(confirmed) = decision else { return };
        let confirm = state.confirm.take().expect("checked above");
        if !confirmed {
            return;
        }
        match confirm {
            Confirm::LeavePicking => state.back_to_define(),
            Confirm::UndoLot(lot) => match inventory_db::delete_bulk_lot(lot.id) {
                Ok(()) => {
                    if state
                        .selected_lot
                        .as_ref()
                        .is_some_and(|(id, _)| *id == lot.id)
                    {
                        state.selected_lot = None;
                    }
                    app_state.mark_db_changed();
                }
                Err(e) => state.history_error = Some(format!("Could not undo “{}”: {e}", lot.name)),
            },
            Confirm::MarkRemoved(lot) => match inventory_db::mark_bulk_lot_removed(lot.id) {
                Ok(()) => app_state.mark_db_changed(),
                Err(e) => {
                    state.history_error = Some(format!("Could not update “{}”: {e}", lot.name))
                }
            },
        }
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
