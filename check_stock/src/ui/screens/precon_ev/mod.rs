//! Precon EV screen.
//!
//! Ranks every Commander precon by expected singles value (EV) against its
//! sealed price, with a per-product detail view (card breakdown and an EV vs.
//! sealed price history chart). Independent of the inventory: the catalog
//! and every price come live from the inventory_sync tracker, decklists from
//! MTGJSON (cached on disk). Changing the cutoff or price field recomputes
//! locally — nothing is fetched again.

mod detail;
mod ranking;

use crate::{
    api::{cardmarket::product_url, inventory_sync::InventorySyncClient, mtgjson::MtgjsonClient},
    cache::{DeckCache, SalesCache},
    precon_ev::{
        build_rows, ev_history, history_start, sales_candidates, sort_rows, trim_leading_gaps,
        weekly_dates, EvParams, PreconData, FOIL_LOOKBACK_DAYS, LIQUID_FULL_SHARE, SALES_DAYS,
        SALES_MIN_PRICE_EUR, SEALED_FIELDS, STALE_SEALED_FACTOR,
    },
    precon_loader::{load_history, load_precons, load_sales, local_date, LoadEvent, LoadOptions},
    ui::{
        components::InventorySyncBar,
        state::{
            AppState, ConnectionStatus, InventoryPriceSource, PreconEvState, PreconLoadMsg, Screen,
        },
        style,
    },
};
use eframe::egui;
use log::info;

pub struct PreconEvScreen;

/// Pricing parameters for the current UI settings.
fn params(state: &PreconEvState) -> EvParams {
    EvParams {
        field: state.field,
        sealed_field: state.sealed_field,
        cutoff: state.cutoff,
        foil_needs_sales: state.foil_needs_sales,
    }
}

/// Link that opens a product's Cardmarket page in the system browser
/// (hovering shows the URL).
fn cardmarket_link(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>, id_product: u64) {
    ui.hyperlink_to(text, product_url(id_product));
}

impl PreconEvScreen {
    pub fn show(ctx: &egui::Context, app_state: &mut AppState, state: &mut PreconEvState) {
        // Everything here needs the tracker; check it right away.
        if matches!(app_state.inventory_sync_status, ConnectionStatus::Unchecked) {
            InventorySyncBar::start_health_check(app_state);
        }
        Self::poll_load(state);
        Self::poll_history(state);
        let connected = matches!(app_state.inventory_sync_status, ConnectionStatus::Connected);
        // Also re-fetches after a reload cleared the histories.
        Self::ensure_history(state, connected, &app_state.inventory_sync_url);
        if state.loading || state.sales_loading || state.history_rx.is_some() {
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::both()
                .id_salt("precon_ev_scroll")
                .show(ui, |ui| {
                    let back = if state.selected.is_some() {
                        "Back to ranking"
                    } else {
                        "Back"
                    };
                    if style::back_button(ui, back) {
                        if state.selected.is_some() {
                            state.selected = None;
                        } else {
                            app_state.current_screen = Screen::Welcome;
                        }
                    }
                    ui.add_space(8.0);
                    style::screen_heading(ui, "Precon EV");

                    Self::show_sync_bar(ui, ctx, app_state, state);
                    ui.add_space(6.0);
                    Self::show_controls(ui, state);
                    ui.add_space(8.0);

                    if let Some(err) = &state.error {
                        style::status_error(ui, err);
                        ui.add_space(6.0);
                    }

                    match (state.selected, state.data.is_some()) {
                        (Some(id), true) => detail::show(ui, state, id),
                        (None, true) => {
                            Self::show_info(ui, state);
                            ui.add_space(6.0);
                            let action = ranking::show_table(ui, state);
                            if action.resorted {
                                sort_rows(&mut state.rows, state.sort, state.sort_desc);
                            }
                            if let Some(id) = action.open {
                                Self::open_detail(state, id);
                            }
                        }
                        _ if !state.loading => {
                            ui.label(
                                egui::RichText::new(
                                    "Connect to the price tracker and load the precons to rank \
                                     every Commander precon by singles value vs. sealed price.",
                                )
                                .size(12.0)
                                .color(style::TEXT_MUTED),
                            );
                        }
                        _ => {}
                    }
                });
        });
    }

    // ── Polling ─────────────────────────────────────────────────────────────

    fn poll_load(state: &mut PreconEvState) {
        let Some(rx) = &state.rx else { return };
        let mut messages = Vec::new();
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok(msg) => messages.push(msg),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        for msg in messages {
            match msg {
                PreconLoadMsg::Event(LoadEvent::Stage(stage)) => state.stage = stage,
                PreconLoadMsg::Event(LoadEvent::Decks { done, total }) => {
                    state.stage = "Loading decklists…".to_string();
                    state.progress = Some(("Decklists", done, total));
                }
                PreconLoadMsg::Event(LoadEvent::Sales { done, total }) => {
                    state.stage = "Fetching sales history (cached for today)…".to_string();
                    state.progress = Some(("Sales history", done, total));
                }
                PreconLoadMsg::Done(Ok(data)) => {
                    info!(
                        "Precon EV: loaded {} products, {} decklists",
                        data.catalog.len(),
                        data.decks.len()
                    );
                    // The ranking shows now; the sales phase keeps the channel.
                    state.loading = false;
                    state.sales_loading = true;
                    state.progress = None;
                    state.stage = "Fetching sales history…".to_string();
                    state.data = Some(*data);
                    // Prices were just refreshed; old histories are stale.
                    state.histories.clear();
                    state.error = None;
                    Self::rebuild(state);
                }
                PreconLoadMsg::Done(Err(e)) => {
                    Self::finish_load(state);
                    state.error = Some(e);
                    return;
                }
                PreconLoadMsg::Sales(result) => {
                    Self::finish_load(state);
                    if let Some(data) = &mut state.data {
                        match result {
                            Ok(map) => {
                                info!("Precon EV: sales stats for {} cards", map.len());
                                data.sales = Some(map);
                                data.sales_error = None;
                            }
                            Err(e) => {
                                data.sales = None;
                                data.sales_error = Some(e);
                            }
                        }
                    }
                    Self::rebuild(state);
                    return;
                }
            }
        }
        if disconnected {
            let in_sales = state.sales_loading;
            Self::finish_load(state);
            match (&mut state.data, in_sales) {
                (Some(data), true) => {
                    data.sales_error = Some("Sales history fetch stopped unexpectedly.".to_string())
                }
                _ => state.error = Some("Precon load stopped unexpectedly.".to_string()),
            }
        }
    }

    fn finish_load(state: &mut PreconEvState) {
        state.loading = false;
        state.sales_loading = false;
        state.sales_cancel = None;
        state.rx = None;
        state.progress = None;
        state.stage.clear();
    }

    fn poll_history(state: &mut PreconEvState) {
        let Some(rx) = &state.history_rx else { return };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("History load stopped unexpectedly.".to_string())
            }
        };
        state.history_rx = None;
        match result {
            Ok(history) => {
                state.histories.insert(history.id_product, history);
                state.history_error = None;
                Self::rebuild_history(state);
            }
            Err(e) => state.history_error = Some(e),
        }
    }

    // ── Panels ──────────────────────────────────────────────────────────────

    fn show_sync_bar(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        app_state: &mut AppState,
        state: &mut PreconEvState,
    ) {
        let url = app_state.inventory_sync_url.clone();
        InventorySyncBar::show(ui, ctx, app_state, |ui, connected| {
            if connected {
                let label = if state.data.is_some() {
                    "Reload"
                } else {
                    "Load precons"
                };
                if style::primary_button_enabled(ui, label, !state.loading)
                    .on_hover_text(
                        "Fetch the precon catalog and current prices (decklists are cached)",
                    )
                    .clicked()
                {
                    Self::spawn_load(state, &url, false);
                }
                if style::secondary_button_enabled(ui, "Refresh MTGJSON data", !state.loading)
                    .on_hover_text(
                        "Re-download MTGJSON's sealed product index (~12 MB). \
                         Happens automatically weekly or when new precons appear.",
                    )
                    .clicked()
                {
                    Self::spawn_load(state, &url, true);
                }
            }
            if state.loading || state.sales_loading {
                ui.spinner();
                ui.label(
                    egui::RichText::new(&state.stage)
                        .color(style::TEXT_MUTED)
                        .size(11.0),
                );
            }
        });
        let busy = state.loading || state.sales_loading;
        if let (true, Some((label, done, total))) = (busy, state.progress) {
            ui.add_space(4.0);
            ui.add(
                egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                    .text(format!("{label} {done}/{total}"))
                    .desired_width(ui.available_width().min(500.0)),
            );
        }
    }

    fn show_controls(ui: &mut egui::Ui, state: &mut PreconEvState) {
        style::section_frame().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Card price:");
                let before = (state.field, state.sealed_field);
                egui::ComboBox::from_id_salt("precon_ev_field")
                    .selected_text(state.field.as_str())
                    .show_ui(ui, |ui| {
                        for src in InventoryPriceSource::all() {
                            ui.selectable_value(&mut state.field, *src, src.as_str());
                        }
                    });

                ui.add_space(12.0);
                ui.label("Sealed price:");
                egui::ComboBox::from_id_salt("precon_ev_sealed_field")
                    .selected_text(state.sealed_field.as_str())
                    .show_ui(ui, |ui| {
                        for src in SEALED_FIELDS {
                            ui.selectable_value(&mut state.sealed_field, src, src.as_str());
                        }
                    })
                    .response
                    .on_hover_text(
                        "Cardmarket publishes only Trend, Low (cheapest offer) and Average for \
                         sealed products — no 1/7/30-day averages.",
                    );

                ui.add_space(12.0);
                ui.label("Bulk cutoff:");
                let cutoff = ui
                    .add(
                        egui::DragValue::new(&mut state.cutoff)
                            .speed(0.05)
                            .range(0.0..=100.0)
                            .fixed_decimals(2)
                            .prefix("€"),
                    )
                    .on_hover_text(
                        "Card copies cheaper than this are left out of EV — they rarely sell. \
                         Top 5 ignores the cutoff.",
                    );

                ui.add_space(12.0);
                let gate = ui
                    .checkbox(&mut state.foil_needs_sales, "Foil premium only if sold")
                    .on_hover_text(format!(
                        "Value a foil at its non-foil price unless the foil sold in the last \
                         {FOIL_LOOKBACK_DAYS} days. Removes foil premiums nobody pays \
                         (Collector's Editions)."
                    ));

                ui.add_space(12.0);
                ui.label("Filter:");
                ui.add(
                    egui::TextEdit::singleline(&mut state.filter)
                        .hint_text("deck or set")
                        .desired_width(160.0),
                );
                ui.add_space(8.0);
                ui.checkbox(
                    &mut state.hide_no_decklist,
                    "Hide products without decklist",
                );

                if before != (state.field, state.sealed_field) || cutoff.changed() || gate.changed()
                {
                    Self::rebuild(state);
                }
            });
        });
    }

    fn show_info(ui: &mut egui::Ui, state: &PreconEvState) {
        let Some(data) = &state.data else { return };
        let ready = state.rows.iter().filter(|r| r.is_ready()).count();
        let index_date = chrono::DateTime::from_timestamp(data.index_fetched_at_unix, 0)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "?".to_string());
        ui.label(
            egui::RichText::new(format!(
                "{} precons · {ready} with decklist · MTGJSON {} · sealed index from {index_date}",
                state.rows.len(),
                data.deck_list_version.as_deref().unwrap_or("?"),
            ))
            .size(11.0)
            .color(style::TEXT_MUTED),
        );
        if data.index_stale {
            style::status_error(
                ui,
                "MTGJSON was unreachable — using the cached sealed index; the newest precons may \
                 show as missing.",
            );
        }
        if state.sales_loading {
            ui.label(
                egui::RichText::new(
                    "Loading sales history in the background — the foil check and Liquid EV \
                     apply when it finishes (first load of the day only).",
                )
                .size(11.0)
                .color(style::TEXT_MUTED),
            );
        }
        if let Some(err) = &data.sales_error {
            style::status_error(
                ui,
                &format!("{err} — foil sales check and Liquid EV are unavailable."),
            );
        }
        if data.catalog_truncated {
            style::status_error(
                ui,
                "The tracker search hit its result limit — some precons may be missing.",
            );
        }
        ui.label(
            egui::RichText::new(format!(
                "EV = card copies priced at or above €{:.2} (card price: {}), vs. the sealed {}. \
                 Excludes tokens, display commanders, sample packs and extras. Liquid EV = EV with \
                 each card weighted by how often it sold over the last {SALES_DAYS} days: full \
                 weight from a sale on {:.0}% of days, less below, nothing if unsold \
                 (market-wide, all countries and languages; cards under \
                 €{SALES_MIN_PRICE_EUR:.2} count as unsellable). Top 5 = five most expensive \
                 copies, regardless of the cutoff. ⚠ = sealed price more than {:.0}% below the \
                 cheapest offer (thin market, ratio overstated).",
                state.cutoff,
                state.field.as_str(),
                state.sealed_field.as_str(),
                LIQUID_FULL_SHARE * 100.0,
                (STALE_SEALED_FACTOR - 1.0) * 100.0,
            ))
            .size(11.0)
            .color(style::TEXT_MUTED),
        );
    }

    // ── Actions ─────────────────────────────────────────────────────────────

    fn spawn_load(state: &mut PreconEvState, url: &str, force_index_refresh: bool) {
        info!("Precon EV: loading from {url} (force index refresh: {force_index_refresh})");
        // A still-running sales fetch from the previous load stops at its
        // next batch instead of loading the tracker for nothing.
        if let Some(cancel) = state.sales_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        state.sales_cancel = Some(cancel.clone());
        state.sales_loading = false;
        let (tx, rx) = std::sync::mpsc::channel();
        state.rx = Some(rx);
        state.loading = true;
        state.error = None;
        state.progress = None;
        state.stage = "Starting…".to_string();
        let opts = LoadOptions {
            tracker: InventorySyncClient::new(url),
            mtgjson: MtgjsonClient::default(),
            force_index_refresh,
            now_unix: chrono::Utc::now().timestamp(),
        };
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let progress = |event| {
                let _ = progress_tx.send(PreconLoadMsg::Event(event));
            };
            let data = match load_precons(&opts, &DeckCache::new(), &progress) {
                Ok(data) => data,
                Err(e) => {
                    let _ = tx.send(PreconLoadMsg::Done(Err(e)));
                    return;
                }
            };
            let candidates = sales_candidates(&data.decks, &data.prices);
            if tx.send(PreconLoadMsg::Done(Ok(Box::new(data)))).is_err() {
                return; // Superseded by a newer load.
            }
            let result = load_sales(
                &opts.tracker,
                &SalesCache::new(),
                &candidates,
                local_date(opts.now_unix),
                &progress,
                &cancel,
            );
            let _ = tx.send(PreconLoadMsg::Sales(result));
        });
    }

    /// Opens the detail view; [`Self::ensure_history`] fetches its history.
    fn open_detail(state: &mut PreconEvState, id: u64) {
        state.selected = Some(id);
        state.history_error = None;
        state.history_rx = None;
        Self::rebuild_history(state);
    }

    /// Starts a history fetch for the open detail view unless one is cached,
    /// in flight, or already failed (no retry loop; reopen to retry).
    fn ensure_history(state: &mut PreconEvState, connected: bool, url: &str) {
        let Some(id) = state.selected else { return };
        if state.histories.contains_key(&id)
            || state.history_rx.is_some()
            || state.history_error.is_some()
        {
            return;
        }
        if !connected {
            state.history_error =
                Some("Connect to the price tracker to load the EV history.".to_string());
            return;
        }
        let Some((card_ids, dates)) = Self::history_request(state, id) else {
            state.history_error = Some("No decklist loaded for this product.".to_string());
            return;
        };
        info!(
            "Precon EV: fetching history for {id} ({} cards, {} dates)",
            card_ids.len(),
            dates.len()
        );
        let (tx, rx) = std::sync::mpsc::channel();
        state.history_rx = Some(rx);
        let client = InventorySyncClient::new(url);
        std::thread::spawn(move || {
            let _ = tx.send(load_history(&client, id, card_ids, dates));
        });
    }

    /// Card ids and weekly dates for a product's history request.
    fn history_request(state: &PreconEvState, id: u64) -> Option<(Vec<u64>, Vec<String>)> {
        let data = state.data.as_ref()?;
        let decks = data.decks_for(id)?;
        let release = state
            .rows
            .iter()
            .find(|r| r.id_product == id)
            .and_then(|r| r.release_date.as_deref());
        let today = chrono::Local::now().date_naive();
        let dates = weekly_dates(history_start(release, today), today);
        let card_ids = decks.iter().flat_map(|d| d.card_ids()).collect();
        Some((card_ids, dates))
    }

    /// Recomputes rows (and the open chart) from the loaded data.
    fn rebuild(state: &mut PreconEvState) {
        let params = params(state);
        let mut rows = state
            .data
            .as_ref()
            .map(|data| build_rows(data, params))
            .unwrap_or_default();
        sort_rows(&mut rows, state.sort, state.sort_desc);
        state.rows = rows;
        Self::rebuild_history(state);
    }

    fn rebuild_history(state: &mut PreconEvState) {
        let params = params(state);
        state.history_points = match (&state.data, state.selected) {
            (Some(data), Some(id)) => Self::history_points(data, state, id, params),
            _ => Vec::new(),
        };
    }

    fn history_points(
        data: &PreconData,
        state: &PreconEvState,
        id: u64,
        params: EvParams,
    ) -> Vec<crate::precon_ev::EvPoint> {
        match (state.histories.get(&id), data.decks_for(id)) {
            (Some(history), Some(decks)) => trim_leading_gaps(ev_history(
                &decks,
                id,
                &history.snapshots,
                &history.dates,
                data.sales.as_ref(),
                params,
            )),
            _ => Vec::new(),
        }
    }
}
