//! Price Movers screen.
//!
//! Read-only: joins the in-stock inventory with 7/30-day market movement from
//! inventory_sync snapshots and surfaces the cards whose market price moved —
//! spikes worth selling into (raise price before the market corrects) and
//! falling knives worth liquidating. The listing-age column ties this to the
//! dead-stock report: old stock that is also losing value is the first
//! liquidation candidate.
//!
//! Only raw snapshot rows are fetched; every delta is computed locally in
//! [`crate::price_trends`], keeping load off the server.

use crate::{
    api::inventory_sync::InventorySyncClient,
    inventory_db::get_in_stock_cards,
    price_trends::{build_stock_movers, SnapshotSet, StockMover},
    ui::{
        components::InventorySyncBar,
        state::{AppState, InventoryPriceSource, MoverDirection, MoverSort, MoversState, Screen},
        style,
    },
};
use eframe::egui;
use log::info;

/// Max rows rendered in the table.
const MAX_ROWS: usize = 300;

pub struct MoversScreen;

impl MoversScreen {
    pub fn show(ctx: &egui::Context, app_state: &mut AppState, state: &mut MoversState) {
        Self::poll_fetch(state);
        if state.loading {
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("movers_scroll")
                .show(ui, |ui| {
                    if style::back_button(ui, "Back") {
                        app_state.current_screen = Screen::Welcome;
                    }
                    ui.add_space(8.0);
                    style::screen_heading(ui, "Price Movers");

                    Self::show_sync_bar(ui, ctx, app_state, state);
                    ui.add_space(6.0);
                    Self::show_controls(ui, state);
                    ui.add_space(10.0);

                    if let Some(err) = &state.error {
                        style::status_error(ui, err);
                        ui.add_space(6.0);
                    }

                    if !state.snapshots.is_empty() {
                        Self::show_table(ui, state);
                    } else if !state.loading {
                        ui.label(
                            egui::RichText::new(
                                "Connect to the inventory_sync server and fetch snapshots to see \
                                 which of your in-stock cards moved in price.",
                            )
                            .size(12.0)
                            .color(style::TEXT_MUTED),
                        );
                    }
                });
        });
    }

    fn poll_fetch(state: &mut MoversState) {
        let Some(rx) = &state.rx else { return };
        match rx.try_recv() {
            Ok(result) => {
                state.loading = false;
                state.rx = None;
                match result {
                    Ok((snapshots, dates)) => {
                        state.snapshots = SnapshotSet::new(&dates, snapshots);
                        state.status = format!(
                            "{} in-stock cards · movement for {} products",
                            state.cards.len(),
                            state.snapshots.len()
                        );
                        state.error = None;
                        Self::rebuild(state);
                    }
                    Err(e) => {
                        state.status = String::new();
                        state.error = Some(format!("Snapshot fetch failed: {e}"));
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                state.loading = false;
                state.rx = None;
            }
        }
    }

    fn show_sync_bar(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        app_state: &mut AppState,
        state: &mut MoversState,
    ) {
        let url = app_state.inventory_sync_url.clone();
        InventorySyncBar::show(ui, ctx, app_state, |ui, connected| {
            if connected {
                let label = if state.loading {
                    "Fetching…"
                } else {
                    "Fetch movement"
                };
                if style::secondary_button(ui, label).clicked() && !state.loading {
                    Self::spawn_fetch(state, &url);
                }
            }
            if state.loading {
                ui.spinner();
            }
            if !state.status.is_empty() {
                ui.label(
                    egui::RichText::new(&state.status)
                        .color(style::TEXT_MUTED)
                        .size(11.0),
                );
            }
        });
    }

    fn show_controls(ui: &mut egui::Ui, state: &mut MoversState) {
        style::section_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Price field:");
                let before_field = state.field;
                egui::ComboBox::from_id_salt("movers_field")
                    .selected_text(state.field.as_str())
                    .show_ui(ui, |ui| {
                        for src in InventoryPriceSource::all() {
                            ui.selectable_value(&mut state.field, *src, src.as_str());
                        }
                    });

                ui.add_space(12.0);
                ui.label("Min price:");
                let min_price = ui.add(
                    egui::DragValue::new(&mut state.min_price)
                        .speed(0.05)
                        .range(0.0..=1000.0)
                        .prefix("€"),
                );

                ui.add_space(12.0);
                ui.label("Min age:");
                let min_age = ui.add(
                    egui::DragValue::new(&mut state.min_age_days)
                        .speed(1)
                        .range(0..=3650)
                        .suffix(" d"),
                );

                ui.add_space(12.0);
                let mut direction_changed = false;
                for d in [
                    MoverDirection::All,
                    MoverDirection::Risers,
                    MoverDirection::Fallers,
                ] {
                    if ui
                        .selectable_label(state.direction == d, d.as_str())
                        .clicked()
                    {
                        state.direction = d;
                        direction_changed = true;
                    }
                }

                if before_field != state.field
                    || min_price.changed()
                    || min_age.changed()
                    || direction_changed
                {
                    Self::rebuild(state);
                }
            });
        });
    }

    fn show_table(ui: &mut egui::Ui, state: &mut MoversState) {
        let mut rows: Vec<&StockMover> = state.movers.iter().collect();

        let cmp_opt =
            |a: Option<f64>, b: Option<f64>| a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
        rows.sort_by(|a, b| {
            let ord = match state.sort {
                MoverSort::Change7 => cmp_opt(a.change.pct_7d, b.change.pct_7d),
                MoverSort::Change30 => cmp_opt(a.change.pct_30d, b.change.pct_30d),
                MoverSort::Age => a.age_days.cmp(&b.age_days),
                MoverSort::Current => cmp_opt(a.change.current, b.change.current),
                MoverSort::Listed => cmp_opt(Some(a.card.price), Some(b.card.price)),
                MoverSort::Name => a.card.name.cmp(&b.card.name),
                MoverSort::Set => a
                    .card
                    .set_code
                    .cmp(&b.card.set_code)
                    .then_with(|| a.card.name.cmp(&b.card.name)),
                MoverSort::Quantity => a.card.quantity.cmp(&b.card.quantity),
                MoverSort::Location => a.card.location.cmp(&b.card.location),
            };
            if state.sort_desc {
                ord.reverse()
            } else {
                ord
            }
        });

        let total = rows.len();
        let shown = total.min(MAX_ROWS);
        ui.label(
            egui::RichText::new(if total > shown {
                format!("Showing top {shown} of {total} movers")
            } else {
                format!("{total} movers")
            })
            .size(11.0)
            .color(style::TEXT_MUTED),
        );
        ui.add_space(2.0);

        let header = |ui: &mut egui::Ui,
                      label: &str,
                      col: MoverSort,
                      sort: &mut MoverSort,
                      desc: &mut bool| {
            let active = *sort == col;
            let arrow = if active {
                if *desc {
                    " \u{25BC}"
                } else {
                    " \u{25B2}"
                }
            } else {
                ""
            };
            let text = egui::RichText::new(format!("{label}{arrow}")).strong();
            if ui
                .add(egui::Label::new(text).sense(egui::Sense::click()))
                .clicked()
            {
                if active {
                    *desc = !*desc;
                } else {
                    *sort = col;
                    *desc = true;
                }
            }
        };

        egui::Grid::new("movers_table")
            .num_columns(9)
            .striped(true)
            .spacing([12.0, 2.0])
            .show(ui, |ui| {
                header(
                    ui,
                    "Card",
                    MoverSort::Name,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Set",
                    MoverSort::Set,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Qty",
                    MoverSort::Quantity,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Age",
                    MoverSort::Age,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Listed",
                    MoverSort::Listed,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Market",
                    MoverSort::Current,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Δ7d",
                    MoverSort::Change7,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Δ30d",
                    MoverSort::Change30,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Location",
                    MoverSort::Location,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                ui.end_row();

                for m in rows.into_iter().take(MAX_ROWS) {
                    let name = if m.card.is_foil {
                        format!("{} ✦", m.card.name)
                    } else {
                        m.card.name.clone()
                    };
                    ui.label(name);
                    ui.label(format!("{} {}", m.card.set_code, m.card.condition));
                    ui.label(format!("×{}", m.card.quantity));
                    ui.label(format!("{} d", m.age_days));
                    ui.label(format!("€{:.2}", m.card.price));
                    ui.label(
                        m.change
                            .current
                            .map(|p| format!("€{p:.2}"))
                            .unwrap_or_else(|| "—".to_string()),
                    );
                    style::change_pct_label(ui, m.change.pct_7d);
                    style::change_pct_label(ui, m.change.pct_30d);
                    ui.label(&m.card.location);
                    ui.end_row();
                }
            });
    }

    // ── Actions ─────────────────────────────────────────────────────────────

    /// Loads the in-stock inventory and fetches raw snapshots for its products.
    fn spawn_fetch(state: &mut MoversState, url: &str) {
        state.cards = match get_in_stock_cards() {
            Ok(c) => c,
            Err(e) => {
                state.error = Some(format!("Failed to read inventory: {e}"));
                return;
            }
        };
        let ids: Vec<u64> = state
            .cards
            .iter()
            .filter_map(|c| c.cardmarket_id.parse::<u64>().ok())
            .collect::<std::collections::HashSet<u64>>()
            .into_iter()
            .collect();
        if ids.is_empty() {
            state.error = Some("No in-stock cards with cardmarket IDs found.".to_string());
            return;
        }

        info!(
            "Movers: fetching snapshots for {} products from {url}",
            ids.len()
        );
        let dates = SnapshotSet::request_dates(chrono::Local::now().date_naive());
        let (tx, rx) = std::sync::mpsc::channel();
        state.rx = Some(rx);
        state.loading = true;
        state.error = None;
        // Instant feedback — the snapshot fetch can take a while on large stocks.
        state.status = format!("Fetching 90-day snapshots for {} products…", ids.len());
        let client = InventorySyncClient::new(url);
        std::thread::spawn(move || {
            let result = client
                .price_snapshots_blocking(&ids, &dates)
                .map(|snapshots| (snapshots, dates))
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }

    /// Rejoins cards × snapshots with the current parameters.
    fn rebuild(state: &mut MoversState) {
        let today = chrono::Local::now().date_naive();
        let mut movers = build_stock_movers(
            &state.cards,
            &state.snapshots,
            state.field,
            today,
            state.min_price,
        );
        movers.retain(|m| {
            if m.age_days < state.min_age_days {
                return false;
            }
            match state.direction {
                MoverDirection::All => true,
                MoverDirection::Risers => {
                    m.change.pct_7d.unwrap_or(0.0) > 0.0 || m.change.pct_30d.unwrap_or(0.0) > 0.0
                }
                MoverDirection::Fallers => {
                    m.change.pct_7d.unwrap_or(0.0) < 0.0 || m.change.pct_30d.unwrap_or(0.0) < 0.0
                }
            }
        });
        state.movers = movers;
    }
}
