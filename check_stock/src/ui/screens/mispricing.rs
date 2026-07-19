//! Mispricing / Margin report screen.
//!
//! Read-only: compares every in-stock card's listed price against a market
//! reference and surfaces under/over-priced listings plus the money
//! implications. It never writes prices anywhere — see the repricing feature
//! request for the write side.
//!
//! The market reference comes from either the inventory_sync server (default —
//! latest collected prices, plus 7/30-day movement columns from raw snapshot
//! rows) or a full Cardmarket price-guide download. All movement deltas are
//! computed locally in [`crate::price_trends`]; the server only serves rows.

use crate::{
    api::cardmarket::PriceGuide,
    api::inventory_sync::{InventorySyncClient, PriceFields},
    inventory_db::{get_in_stock_cards, InStockCard},
    mispricing::{build_report, Action, MarketData, MispricingReport, PriceVerdict},
    price_trends::{SnapshotSet, TrendChange},
    ui::{
        components::InventorySyncBar,
        state::{
            AppState, FetchMsg, InventoryPriceSource, MarketSource, MispricingSort,
            MispricingState, Screen, VerdictFilter,
        },
        style,
    },
};
use eframe::egui;
use log::info;

/// Max rows rendered in the table (sorted so the highest-impact are on top).
const MAX_ROWS: usize = 300;

pub struct MispricingScreen;

impl MispricingScreen {
    pub fn show(ctx: &egui::Context, app_state: &mut AppState, state: &mut MispricingState) {
        Self::poll_guide_fetch(state);
        Self::poll_sync_fetch(state);
        if state.guide_loading || state.sync_loading {
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("mispricing_scroll")
                .show(ui, |ui| {
                    if style::back_button(ui, "Back") {
                        app_state.current_screen = Screen::Welcome;
                    }
                    ui.add_space(8.0);
                    style::screen_heading(ui, "Mispricing / Margin Report");

                    Self::show_sync_bar(ui, ctx, app_state, state);
                    ui.add_space(6.0);
                    Self::show_controls(ui, state);
                    ui.add_space(10.0);

                    if let Some(err) = &state.error {
                        style::status_error(ui, err);
                        ui.add_space(6.0);
                    }

                    if let Some(report) = state.report.clone() {
                        Self::show_summary(ui, &report);
                        ui.add_space(8.0);
                        Self::show_consistency(ui, state);
                        ui.add_space(8.0);
                        Self::show_table(ui, state, &report);
                    }
                });
        });
    }

    /// Drains the background price-guide fetch channel and rebuilds the report
    /// when the guide arrives.
    fn poll_guide_fetch(state: &mut MispricingState) {
        let Some(rx) = &state.guide_rx else { return };
        match rx.try_recv() {
            Ok(result) => {
                state.guide_loading = false;
                state.guide_rx = None;
                match result {
                    Ok(guide) => {
                        state.guide_status =
                            format!("Price guide loaded ({} entries)", guide.len());
                        state.price_guide = Some(guide);
                        Self::rebuild(state);
                    }
                    Err(e) => {
                        state.guide_status = String::new();
                        state.error = Some(format!("Price guide fetch failed: {e}"));
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                state.guide_loading = false;
                state.guide_rx = None;
            }
        }
    }

    /// Drains the background inventory_sync fetch channel — progress messages
    /// update the status line live; the final result rebuilds the report.
    fn poll_sync_fetch(state: &mut MispricingState) {
        let Some(rx) = state.sync_rx.take() else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(FetchMsg::Progress(msg)) => state.sync_status = msg,
                Ok(FetchMsg::Done(result)) => {
                    state.sync_loading = false;
                    match result {
                        Ok((latest, snapshots, dates)) => {
                            state.inventory_prices =
                                latest.into_iter().map(|p| (p.id_product, p)).collect();
                            state.snapshots = SnapshotSet::new(&dates, snapshots);
                            state.sync_status = format!(
                                "{} prices · movement for {} products",
                                state.inventory_prices.len(),
                                state.snapshots.len()
                            );
                            state.error = None;
                            Self::rebuild(state);
                        }
                        Err(e) => {
                            state.sync_status = String::new();
                            state.error = Some(format!("Inventory sync fetch failed: {e}"));
                        }
                    }
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Still in flight — keep the receiver for the next frame.
                    state.sync_rx = Some(rx);
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    state.sync_loading = false;
                    return;
                }
            }
        }
    }

    fn show_sync_bar(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        app_state: &mut AppState,
        state: &mut MispricingState,
    ) {
        let url = app_state.inventory_sync_url.clone();
        InventorySyncBar::show(ui, ctx, app_state, |ui, connected| {
            if connected {
                let label = if state.sync_loading {
                    "Fetching…"
                } else {
                    "Fetch prices"
                };
                if style::secondary_button(ui, label).clicked() && !state.sync_loading {
                    Self::spawn_sync_fetch(state, &url);
                }
            }
            if state.sync_loading {
                ui.spinner();
            }
            if !state.sync_status.is_empty() {
                ui.label(
                    egui::RichText::new(&state.sync_status)
                        .color(style::TEXT_MUTED)
                        .size(11.0),
                );
            }
        });
    }

    fn show_controls(ui: &mut egui::Ui, state: &mut MispricingState) {
        style::section_frame().show(ui, |ui| {
            // Market-source row.
            ui.horizontal(|ui| {
                ui.label("Market source:");
                egui::ComboBox::from_id_salt("mispricing_market_source")
                    .selected_text(state.source.as_str())
                    .show_ui(ui, |ui| {
                        for src in MarketSource::all() {
                            ui.selectable_value(&mut state.source, *src, src.as_str());
                        }
                    });

                if state.source == MarketSource::PriceGuide {
                    ui.add_space(12.0);
                    let fetch = style::secondary_button_enabled(
                        ui,
                        "Fetch price guide",
                        !state.guide_loading,
                    );
                    if fetch.clicked() {
                        Self::spawn_guide_fetch(state);
                    }
                    if style::secondary_button(ui, "Load from file…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("JSON", &["json"])
                            .pick_file()
                        {
                            Self::load_from_file(state, &path.to_string_lossy());
                        }
                    }
                    if state.guide_loading {
                        ui.spinner();
                        ui.label("Fetching ~50 MB price guide…");
                    } else if !state.guide_status.is_empty() {
                        ui.label(
                            egui::RichText::new(&state.guide_status).color(style::COLOR_SUCCESS),
                        );
                    }
                }
            });

            ui.add_space(6.0);

            // Parameters row.
            ui.horizontal(|ui| {
                ui.label("Reference price:");
                egui::ComboBox::from_id_salt("mispricing_ref_source")
                    .selected_text(state.ref_source.as_str())
                    .show_ui(ui, |ui| {
                        for src in InventoryPriceSource::all() {
                            ui.selectable_value(&mut state.ref_source, *src, src.as_str());
                        }
                    });

                ui.add_space(12.0);
                ui.label("Fair band ±");
                ui.add(
                    egui::DragValue::new(&mut state.threshold_pct)
                        .speed(0.5)
                        .range(0.0..=100.0)
                        .suffix("%"),
                );

                ui.add_space(12.0);
                let can_run = !state.guide_loading
                    && !state.sync_loading
                    && match state.source {
                        MarketSource::PriceGuide => state.price_guide.is_some(),
                        MarketSource::InventorySync => !state.inventory_prices.is_empty(),
                    };
                if style::primary_button_enabled(ui, "Analyse", can_run).clicked() {
                    Self::rebuild(state);
                }
            });

            let needs_data_hint = match state.source {
                MarketSource::PriceGuide => state.price_guide.is_none() && !state.guide_loading,
                MarketSource::InventorySync => {
                    state.inventory_prices.is_empty() && !state.sync_loading
                }
            };
            if needs_data_hint {
                ui.add_space(4.0);
                let hint = match state.source {
                    MarketSource::PriceGuide => {
                        "Fetch or load the Cardmarket price guide to compare against your listings."
                    }
                    MarketSource::InventorySync => {
                        "Connect to the inventory_sync server and fetch prices to compare against your listings."
                    }
                };
                ui.label(
                    egui::RichText::new(hint)
                        .size(11.0)
                        .color(style::TEXT_MUTED),
                );
            }
        });
    }

    fn show_summary(ui: &mut egui::Ui, r: &MispricingReport) {
        style::section_frame().show(ui, |ui| {
            ui.label(
                egui::RichText::new("Summary")
                    .strong()
                    .size(14.0)
                    .color(style::TEXT_PRIMARY),
            );
            ui.add_space(4.0);
            egui::Grid::new("mispricing_summary")
                .num_columns(4)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Underpriced:").strong());
                    ui.label(format!(
                        "{} listings · ×{}",
                        r.underpriced_rows, r.underpriced_copies
                    ));
                    ui.label(egui::RichText::new("Upside:").strong());
                    ui.label(
                        egui::RichText::new(format!("€{:.2}", r.underpriced_upside))
                            .color(style::COLOR_SUCCESS),
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("Overpriced:").strong());
                    ui.label(format!(
                        "{} listings · ×{}",
                        r.overpriced_rows, r.overpriced_copies
                    ));
                    ui.label(egui::RichText::new("Above market:").strong());
                    ui.label(
                        egui::RichText::new(format!("€{:.2}", r.overpriced_excess))
                            .color(style::COLOR_ERROR),
                    );
                    ui.end_row();

                    ui.label(egui::RichText::new("Fair:").strong());
                    ui.label(format!("{} listings", r.fair_rows));
                    ui.label(egui::RichText::new("No market data:").strong());
                    ui.label(format!("{} listings", r.no_data_rows));
                    ui.end_row();

                    ui.label(egui::RichText::new("Act now:").strong());
                    ui.label(format!(
                        "{} raise · {} cut",
                        r.raise_now_rows, r.cut_now_rows
                    ));
                    ui.label(egui::RichText::new("Stale market data:").strong());
                    ui.label(format!("{} listings", r.stale_rows));
                    ui.end_row();

                    ui.label(egui::RichText::new("Listed value*:").strong());
                    ui.label(format!("€{:.2}", r.total_listed_value));
                    ui.label(egui::RichText::new("Market value*:").strong());
                    ui.label(format!("€{:.2}", r.total_market_value));
                    ui.end_row();
                });
            ui.label(
                egui::RichText::new("* comparable subset only (cards with market data)")
                    .size(10.0)
                    .color(style::TEXT_MUTED),
            );
        });
    }

    /// Collapsible list of contradictions between our own listings — priced
    /// wrong relative to each other, independent of any market reference.
    fn show_consistency(ui: &mut egui::Ui, state: &MispricingState) {
        const MAX_ISSUES: usize = 100;
        let issues = &state.consistency;
        let header = if issues.is_empty() {
            "Internal consistency ✓ (no conflicts)".to_string()
        } else {
            format!("Internal consistency — {} conflicts", issues.len())
        };
        egui::CollapsingHeader::new(egui::RichText::new(header).strong().color(
            if issues.is_empty() {
                style::COLOR_SUCCESS
            } else {
                style::COLOR_ERROR
            },
        ))
        .id_salt("mispricing_consistency")
        .default_open(false)
        .show(ui, |ui| {
            if issues.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "No listings contradict each other (condition order, foil premium, \
                         duplicate prices).",
                    )
                    .size(11.0)
                    .color(style::TEXT_MUTED),
                );
                return;
            }
            if issues.len() > MAX_ISSUES {
                ui.label(
                    egui::RichText::new(format!("Showing first {MAX_ISSUES} conflicts"))
                        .size(11.0)
                        .color(style::TEXT_MUTED),
                );
            }
            egui::Grid::new("mispricing_consistency_table")
                .num_columns(3)
                .striped(true)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Type").strong());
                    ui.label(egui::RichText::new("Card").strong());
                    ui.label(egui::RichText::new("Listings").strong());
                    ui.end_row();
                    for issue in issues.iter().take(MAX_ISSUES) {
                        ui.label(
                            egui::RichText::new(issue.kind.as_str()).color(style::COLOR_ERROR),
                        );
                        ui.label(format!("{} ({})", issue.name, issue.set_code));
                        ui.label(&issue.details);
                        ui.end_row();
                    }
                });
        });
    }

    fn show_table(ui: &mut egui::Ui, state: &mut MispricingState, report: &MispricingReport) {
        // Filter buttons.
        ui.horizontal(|ui| {
            ui.label("Show:");
            for f in [
                VerdictFilter::All,
                VerdictFilter::Underpriced,
                VerdictFilter::Overpriced,
                VerdictFilter::NoData,
            ] {
                if ui.selectable_label(state.filter == f, f.as_str()).clicked() {
                    state.filter = f;
                }
            }
        });
        ui.add_space(4.0);

        // Filter, join each row with its market movement, then sort.
        let ref_source = state.ref_source;
        let change_of = |c: &crate::mispricing::MispricedCard| -> TrendChange {
            c.cardmarket_id
                .parse::<u64>()
                .map(|id| state.snapshots.change(id, ref_source, c.is_foil))
                .unwrap_or_default()
        };
        let mut rows: Vec<(&crate::mispricing::MispricedCard, TrendChange)> = report
            .rows
            .iter()
            .filter(|c| match state.filter {
                VerdictFilter::All => true,
                VerdictFilter::Underpriced => c.verdict == PriceVerdict::Underpriced,
                VerdictFilter::Overpriced => c.verdict == PriceVerdict::Overpriced,
                VerdictFilter::NoData => c.verdict == PriceVerdict::NoMarketData,
            })
            .map(|c| (c, change_of(c)))
            .collect();

        let impact = |c: &crate::mispricing::MispricedCard| c.delta_abs.abs() * c.quantity as f64;
        let cmp_opt = |a: Option<f64>, b: Option<f64>| {
            // Missing values sort below any present value.
            a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
        };
        rows.sort_by(|(a, ca), (b, cb)| {
            let ord = match state.sort {
                MispricingSort::Priority => a
                    .priority
                    .partial_cmp(&b.priority)
                    .unwrap_or(std::cmp::Ordering::Equal),
                MispricingSort::Age => a.age_days.cmp(&b.age_days),
                MispricingSort::Impact => impact(a)
                    .partial_cmp(&impact(b))
                    .unwrap_or(std::cmp::Ordering::Equal),
                MispricingSort::DeltaPct => a
                    .delta_pct
                    .partial_cmp(&b.delta_pct)
                    .unwrap_or(std::cmp::Ordering::Equal),
                MispricingSort::Listed => a
                    .listed_price
                    .partial_cmp(&b.listed_price)
                    .unwrap_or(std::cmp::Ordering::Equal),
                MispricingSort::Market => a
                    .market_price
                    .unwrap_or(0.0)
                    .partial_cmp(&b.market_price.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal),
                MispricingSort::Change7 => cmp_opt(ca.pct_7d, cb.pct_7d),
                MispricingSort::Change30 => cmp_opt(ca.pct_30d, cb.pct_30d),
                MispricingSort::Name => a.name.cmp(&b.name),
                MispricingSort::Set => a
                    .set_code
                    .cmp(&b.set_code)
                    .then_with(|| a.name.cmp(&b.name)),
                MispricingSort::Quantity => a.quantity.cmp(&b.quantity),
                MispricingSort::Verdict => a.verdict.cmp(&b.verdict),
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
                format!("Showing top {shown} of {total} listings")
            } else {
                format!("{total} listings")
            })
            .size(11.0)
            .color(style::TEXT_MUTED),
        );
        ui.add_space(2.0);

        let header = |ui: &mut egui::Ui,
                      label: &str,
                      col: MispricingSort,
                      sort: &mut MispricingSort,
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

        egui::Grid::new("mispricing_table")
            .num_columns(12)
            .striped(true)
            .spacing([12.0, 2.0])
            .show(ui, |ui| {
                header(
                    ui,
                    "Card",
                    MispricingSort::Name,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Set",
                    MispricingSort::Set,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Qty",
                    MispricingSort::Quantity,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Age",
                    MispricingSort::Age,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Listed",
                    MispricingSort::Listed,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Market",
                    MispricingSort::Market,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Δ7d",
                    MispricingSort::Change7,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Δ30d",
                    MispricingSort::Change30,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Δ%",
                    MispricingSort::DeltaPct,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Impact",
                    MispricingSort::Impact,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Verdict",
                    MispricingSort::Verdict,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Action",
                    MispricingSort::Priority,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                ui.end_row();

                let threshold = state.threshold_pct;
                for (c, change) in rows.into_iter().take(MAX_ROWS) {
                    let name = if c.is_foil {
                        format!("{} ✦", c.name)
                    } else {
                        c.name.clone()
                    };
                    ui.label(name);
                    ui.label(format!("{} {}", c.set_code, c.condition));
                    ui.label(format!("×{}", c.quantity));
                    ui.label(format!("{}d", c.age_days));
                    ui.label(format!("€{:.2}", c.listed_price));
                    let adjusted = c.condition_factor < 1.0 && c.market_price.is_some();
                    let mut market_text = c
                        .market_price
                        .map(|m| format!("€{m:.2}"))
                        .unwrap_or_else(|| "—".to_string());
                    let mut market_hint = String::new();
                    if adjusted {
                        market_text.push('*');
                        market_hint.push_str(&format!(
                            "Reference ×{:.2} for {} condition (guide price ≈ NM).\n",
                            c.condition_factor, c.condition
                        ));
                    }
                    if c.stale {
                        market_text.push_str(" ⚠");
                        market_hint.push_str(
                            "Market data is more than a week old — treat with caution.\n",
                        );
                    }
                    let market_label = if c.stale {
                        ui.label(egui::RichText::new(market_text).color(style::TEXT_MUTED))
                    } else {
                        ui.label(market_text)
                    };
                    if !market_hint.is_empty() {
                        market_label.on_hover_text(market_hint.trim_end());
                    }
                    style::change_pct_label(ui, change.pct_7d);
                    style::change_pct_label(ui, change.pct_30d);
                    let (color, verdict_color) = verdict_colors(c.verdict);
                    if c.market_price.is_some() {
                        let delta = ui.label(
                            egui::RichText::new(format!("{:+.0}%", c.delta_pct)).color(color),
                        );
                        if c.effective_threshold_pct > threshold {
                            delta.on_hover_text(format!(
                                "Fair band widened to ±{:.0}% by this card's volatility",
                                c.effective_threshold_pct
                            ));
                        }
                        ui.label(format!("€{:.2}", c.delta_abs.abs() * c.quantity as f64));
                    } else {
                        ui.label("—");
                        ui.label("—");
                    }
                    let mut verdict_text = c.verdict.as_str().to_string();
                    let mut verdict_hint = String::new();
                    if c.below_low {
                        verdict_text.push_str(" ▼low");
                        verdict_hint.push_str("Listed below the market's cheapest listing.\n");
                    }
                    if c.verdict == PriceVerdict::Underpriced && c.recently_listed() {
                        verdict_text.push_str(" ·new");
                        verdict_hint.push_str("Listed within the last day — check for a typo.\n");
                    }
                    if c.language_flagged {
                        verdict_text.push_str(" ·lang");
                        verdict_hint.push_str(&format!(
                            "{} copy — the language-blind reference likely overstates its \
                             value; underpriced verdicts are doubtful.\n",
                            c.language
                        ));
                    }
                    let verdict_label =
                        ui.label(egui::RichText::new(verdict_text).color(verdict_color));
                    if !verdict_hint.is_empty() {
                        verdict_label.on_hover_text(verdict_hint.trim_end());
                    }
                    action_label(ui, c.action);
                    ui.end_row();
                }
            });
    }

    // ── Actions ─────────────────────────────────────────────────────────────

    fn spawn_guide_fetch(state: &mut MispricingState) {
        let (tx, rx) = std::sync::mpsc::channel();
        state.guide_rx = Some(rx);
        state.guide_loading = true;
        state.error = None;
        state.guide_status = String::new();
        std::thread::spawn(move || {
            let result = PriceGuide::fetch_blocking().map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }

    /// Fetches latest prices + 7/30-day snapshots for all in-stock cards from
    /// the inventory_sync server. Only raw rows cross the wire; deltas are
    /// derived locally.
    fn spawn_sync_fetch(state: &mut MispricingState, url: &str) {
        let ids: Vec<u64> = match get_in_stock_cards() {
            Ok(cards) => cards
                .iter()
                .filter_map(|c| c.cardmarket_id.parse::<u64>().ok())
                .collect::<std::collections::HashSet<u64>>()
                .into_iter()
                .collect(),
            Err(e) => {
                state.error = Some(format!("Failed to read inventory: {e}"));
                return;
            }
        };
        if ids.is_empty() {
            state.error = Some("No in-stock cards with cardmarket IDs found.".to_string());
            return;
        }

        info!(
            "Mispricing: fetching prices + snapshots for {} products from {url}",
            ids.len()
        );
        let dates = SnapshotSet::request_dates(chrono::Local::now().date_naive());
        let (tx, rx) = std::sync::mpsc::channel();
        state.sync_rx = Some(rx);
        state.sync_loading = true;
        state.error = None;
        // Instant feedback before the worker thread even starts.
        state.sync_status = format!("Contacting server ({} products)…", ids.len());
        let client = InventorySyncClient::new(url);
        std::thread::spawn(move || {
            let result = (|| {
                let _ = tx.send(FetchMsg::Progress(format!(
                    "Fetching latest prices for {} products…",
                    ids.len()
                )));
                let latest = client
                    .latest_prices_blocking(&ids)
                    .map_err(|e| e.to_string())?;
                let _ = tx.send(FetchMsg::Progress(format!(
                    "{} prices received · fetching 90-day snapshots…",
                    latest.len()
                )));
                let snapshots = client
                    .price_snapshots_blocking(&ids, &dates)
                    .map_err(|e| e.to_string())?;
                Ok((latest, snapshots, dates))
            })();
            let _ = tx.send(FetchMsg::Done(result));
        });
    }

    fn load_from_file(state: &mut MispricingState, path: &str) {
        state.guide_path = path.to_string();
        match PriceGuide::load(path) {
            Ok(guide) => {
                state.guide_status = format!("Price guide loaded ({} entries)", guide.len());
                state.price_guide = Some(guide);
                state.error = None;
                Self::rebuild(state);
            }
            Err(e) => {
                state.error = Some(format!("Failed to load price guide: {e}"));
            }
        }
    }

    /// Rebuilds the report from the current DB inventory + the active market source.
    fn rebuild(state: &mut MispricingState) {
        let cards = match get_in_stock_cards() {
            Ok(c) => c,
            Err(e) => {
                state.error = Some(format!("Failed to read inventory: {e}"));
                return;
            }
        };
        let src = state.ref_source;
        let today = chrono::Local::now().date_naive();
        state.consistency = crate::consistency::find_issues(&cards);
        let snapshots = &state.snapshots;
        let report = match state.source {
            MarketSource::PriceGuide => {
                let Some(guide) = &state.price_guide else {
                    return;
                };
                build_report(&cards, state.threshold_pct, today, |c: &InStockCard| {
                    let Ok(id) = c.cardmarket_id.parse::<u64>() else {
                        return MarketData::default();
                    };
                    let Some(entry) = guide.get(id) else {
                        return MarketData::default();
                    };
                    // The guide download has no row date; volatility still
                    // works when snapshots were fetched from inventory_sync.
                    market_data_of(
                        entry,
                        src,
                        c.is_foil,
                        None,
                        snapshots.volatility_pct(id, src, c.is_foil),
                    )
                })
            }
            MarketSource::InventorySync => {
                if state.inventory_prices.is_empty() {
                    return;
                }
                build_report(&cards, state.threshold_pct, today, |c: &InStockCard| {
                    let Ok(id) = c.cardmarket_id.parse::<u64>() else {
                        return MarketData::default();
                    };
                    let Some(row) = state.inventory_prices.get(&id) else {
                        return MarketData::default();
                    };
                    let price_date =
                        chrono::NaiveDate::parse_from_str(&row.price_date, "%Y-%m-%d").ok();
                    market_data_of(
                        row,
                        src,
                        c.is_foil,
                        price_date,
                        snapshots.volatility_pct(id, src, c.is_foil),
                    )
                })
            }
        };
        state.error = None;
        state.report = Some(report);
    }
}

/// Resolves the full [`MarketData`] for one card from any row carrying the
/// standard 12 Cardmarket price columns.
fn market_data_of<P: PriceFields>(
    row: &P,
    src: InventoryPriceSource,
    is_foil: bool,
    price_date: Option<chrono::NaiveDate>,
    volatility_pct: Option<f64>,
) -> MarketData {
    MarketData {
        reference: row.price_for(src, is_foil),
        low: row.price_for(InventoryPriceSource::Low, is_foil),
        avg1: row.price_for(InventoryPriceSource::Avg1, is_foil),
        avg7: row.price_for(InventoryPriceSource::Avg7, is_foil),
        avg30: row.price_for(InventoryPriceSource::Avg30, is_foil),
        price_date,
        volatility_pct,
    }
}

/// Renders the Action cell: urgent actions bold, direction color-coded.
fn action_label(ui: &mut egui::Ui, action: Action) {
    let color = match action {
        Action::RaiseNow | Action::Raise => style::COLOR_SUCCESS,
        Action::CutNow | Action::Cut => style::COLOR_ERROR,
        Action::Watch | Action::Hold | Action::None => style::TEXT_MUTED,
    };
    let mut text = egui::RichText::new(action.as_str()).color(color);
    if matches!(action, Action::RaiseNow | Action::CutNow) {
        text = text.strong();
    }
    ui.label(text);
}

/// Returns `(delta_color, verdict_color)` for a verdict.
fn verdict_colors(v: PriceVerdict) -> (egui::Color32, egui::Color32) {
    match v {
        // Underpriced: listed below market → green opportunity.
        PriceVerdict::Underpriced => (style::COLOR_SUCCESS, style::COLOR_SUCCESS),
        PriceVerdict::Overpriced => (style::COLOR_ERROR, style::COLOR_ERROR),
        PriceVerdict::Fair => (style::TEXT_MUTED, style::TEXT_MUTED),
        PriceVerdict::NoMarketData => (style::TEXT_MUTED, style::TEXT_MUTED),
    }
}
