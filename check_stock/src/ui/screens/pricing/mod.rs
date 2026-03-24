#[cfg(test)]
#[path = "tests.rs"]
mod tests;

mod constants;
mod draw;
mod eval;
mod geometry;
mod interaction;
mod params;
mod preview;
mod toolbar;

use crate::{
    io::read_csv,
    ui::{
        components::{FilePicker, OutputWindow},
        state::{AppState, ConnectionStatus, NodeId, NodeKind, PricingState, Screen},
        style,
    },
};
use constants::{CANVAS_BG, WIRE_COLOR, WIRE_PENDING_COLOR};
use draw::{draw_bezier, draw_grid, draw_node_chrome};
use eframe::egui;
use eval::evaluate_all;
use geometry::{in_port_pos, node_size, out_port_pos};
use interaction::handle_interactions;
use log::{error, info};
use params::show_node_params;
use preview::{show_preview_window, sort_preview};
use std::collections::HashMap;
use toolbar::{show_add_toolbar, show_save_load_toolbar};

// Re-exported for the tests submodule via `super::<name>`
#[cfg(test)]
use eval::{evaluate_counts, filter_indices};
#[cfg(test)]
use preview::condition_rank;

// ── Screen ────────────────────────────────────────────────────────────────────

pub struct PricingScreen;

impl PricingScreen {
    pub fn show(ctx: &egui::Context, app_state: &mut AppState, state: &mut PricingState) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if style::back_button(ui, "Back") {
                app_state.current_screen = Screen::Welcome;
            }
            ui.add_space(8.0);
            style::screen_heading(ui, "Stock Pricing");

            // ── CSV picker ───────────────────────────────────────────────────
            style::section_frame().show(ui, |ui| {
                let browsed = FilePicker::new("CSV File:", &mut state.csv_path)
                    .with_filter("CSV", &["csv"])
                    .show(ui);
                ui.add_space(6.0);
                if browsed && !state.csv_path.is_empty() {
                    Self::load_csv(state);
                }
                if let Some(err) = &state.load_error {
                    ui.add_space(4.0);
                    style::status_error(ui, err);
                }
            });

            if !state.cards.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("Loaded {} cards", state.cards.len()))
                        .color(style::TEXT_MUTED)
                        .size(12.0),
                );
            }

            ui.add_space(6.0);

            // ── Inventory Sync connection bar ─────────────────────────────────
            show_inventory_sync_bar(ui, ctx, state);

            ui.add_space(4.0);

            // ── Graph save / load ────────────────────────────────────────────
            show_save_load_toolbar(ui, state);

            ui.add_space(2.0);

            // ── Add-node toolbar (only when CSV loaded) ──────────────────────
            ui.add_enabled_ui(!state.cards.is_empty(), |ui| {
                show_add_toolbar(ui, &mut state.graph);
            });

            ui.add_space(4.0);

            // ── Node editor canvas ───────────────────────────────────────────
            show_canvas(ui, ctx, state);
        });

        // ── Output preview window (floating, closeable) ───────────────────
        if state.show_preview {
            show_preview_window(ctx, state);
        }

        // ── Diff CSV output window ───────────────────────────────────────
        if state.show_diff_output {
            OutputWindow::new(
                "Price Diff CSV",
                &mut state.diff_output_content,
                &mut state.show_diff_output,
                "csv",
            )
            .show(ctx);
        }
    }

    fn load_csv(state: &mut PricingState) {
        info!("Loading CSV for pricing: {}", state.csv_path);
        state.load_error = None;
        match read_csv(&state.csv_path) {
            Ok(cards) => {
                info!("Loaded {} cards for pricing", cards.len());
                state.cards = cards;
            }
            Err(e) => {
                error!("Error loading CSV for pricing: {}", e);
                state.load_error = Some(e.to_string());
            }
        }
    }
}

// ── Canvas ────────────────────────────────────────────────────────────────────

fn show_canvas(ui: &mut egui::Ui, ctx: &egui::Context, state: &mut PricingState) {
    let (canvas_response, mut painter) =
        ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
    let canvas_rect = canvas_response.rect;
    let zoom = state.graph.canvas_zoom;

    // Background + dot grid
    painter.rect_filled(canvas_rect, egui::CornerRadius::ZERO, CANVAS_BG);
    draw_grid(&painter, canvas_rect, state.graph.canvas_offset, zoom);

    // Clip everything to canvas bounds
    painter.set_clip_rect(canvas_rect);

    // Precompute node screen-space rects applying zoom and pan
    let rects: Vec<(NodeId, egui::Rect)> = state
        .graph
        .nodes
        .iter()
        .map(|n| {
            let sp = canvas_rect.min + n.pos.to_vec2() * zoom + state.graph.canvas_offset;
            (
                n.id,
                egui::Rect::from_min_size(sp, node_size(&n.kind) * zoom),
            )
        })
        .collect();

    // Evaluate graph once — derive per-node counts and cache output-node indices + price overrides
    let mut all_outputs = evaluate_all(
        &state.graph.nodes,
        &state.graph.wires,
        &state.cards,
        &state.inventory_prices,
    );
    let counts: HashMap<NodeId, usize> = all_outputs
        .iter()
        .map(|(&id, out)| (id, out.indices.len()))
        .collect();
    // Cache the output node's result for the preview window (zero-cost when preview is closed)
    let output_id = state
        .graph
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Output))
        .map(|n| n.id);
    if let Some(out) = output_id.and_then(|id| all_outputs.remove(&id)) {
        state.cached_output = out.indices;
        state.cached_price_overrides = out.overrides;
    } else {
        state.cached_output = Vec::new();
        state.cached_price_overrides = HashMap::new();
    }
    // Apply the current sort in-place so the preview window is always ready
    if let Some(col) = state.preview_sort_col {
        sort_preview(
            &mut state.cached_output,
            &state.cards,
            &state.cached_price_overrides,
            col,
            state.preview_sort_asc,
        );
    }

    // Draw committed wires (behind nodes)
    for wire in &state.graph.wires {
        let fr = rects
            .iter()
            .find(|(id, _)| *id == wire.from_node)
            .map(|(_, r)| *r);
        let tr = rects
            .iter()
            .find(|(id, _)| *id == wire.to_node)
            .map(|(_, r)| *r);
        if let (Some(fr), Some(tr)) = (fr, tr) {
            draw_bezier(
                &painter,
                out_port_pos(fr, wire.from_port, zoom),
                in_port_pos(tr, wire.to_port, zoom),
                WIRE_COLOR,
                2.0,
            );
        }
    }

    // Draw pending wire following the cursor
    if let Some((from_id, from_port)) = state.graph.pending_wire {
        if let Some(fr) = rects.iter().find(|(id, _)| *id == from_id).map(|(_, r)| *r) {
            let start = out_port_pos(fr, from_port, zoom);
            let end = canvas_response.hover_pos().unwrap_or(start);
            draw_bezier(&painter, start, end, WIRE_PENDING_COLOR, 1.5);
            ctx.request_repaint();
        }
    }

    // Draw node chrome (background, header, port circles, labels + count)
    for node in &state.graph.nodes {
        if let Some(rect) = rects.iter().find(|(id, _)| *id == node.id).map(|(_, r)| *r) {
            let is_selected = state.graph.selected.contains(&node.id);
            draw_node_chrome(
                &painter,
                node,
                rect,
                counts.get(&node.id).copied(),
                zoom,
                is_selected,
            );
        }
    }

    // Draw marquee rectangle on top of nodes
    if let Some((start, end)) = state.graph.marquee {
        let sel_rect = egui::Rect::from_two_pos(start, end);
        painter.rect(
            sel_rect,
            egui::CornerRadius::ZERO,
            egui::Color32::from_rgba_premultiplied(80, 140, 255, 20),
            egui::Stroke::new(1.0, egui::Color32::from_rgb(100, 160, 255)),
            egui::StrokeKind::Inside,
        );
    }

    // Parameter widgets sit inside node bodies (ui.put uses screen-space rects)
    for node in &mut state.graph.nodes {
        if let Some(rect) = rects.iter().find(|(id, _)| *id == node.id).map(|(_, r)| *r) {
            show_node_params(ui, node, rect, zoom);
        }
    }

    handle_interactions(
        ctx,
        &canvas_response,
        &rects,
        &mut state.graph,
        canvas_rect,
        zoom,
    );
}

// ── Inventory Sync connection bar ────────────────────────────────────────────

fn show_inventory_sync_bar(ui: &mut egui::Ui, ctx: &egui::Context, state: &mut PricingState) {
    // Poll health-check channel
    if let Some(rx) = &state.health_rx {
        if let Ok(result) = rx.try_recv() {
            match &result {
                Ok(()) => info!(
                    "Inventory sync health check succeeded ({})",
                    state.inventory_sync_url
                ),
                Err(e) => error!(
                    "Inventory sync health check failed ({}): {e}",
                    state.inventory_sync_url
                ),
            }
            match result {
                Ok(()) => state.connection_status = ConnectionStatus::Connected,
                Err(e) => state.connection_status = ConnectionStatus::Failed(e),
            }
            state.health_rx = None;
        }
    }

    // Poll bulk-prices channel
    if let Some(rx) = &state.prices_rx {
        if let Ok(result) = rx.try_recv() {
            match result {
                Ok(prices) => {
                    let count = prices.len();
                    for (id, cached) in prices {
                        state.inventory_prices.insert(id, cached);
                    }
                    info!(
                        "Inventory sync price fetch succeeded: received {count} prices ({})",
                        state.inventory_sync_url
                    );
                }
                Err(e) => {
                    error!(
                        "Inventory sync price fetch failed ({}): {e}",
                        state.inventory_sync_url
                    );
                    state.load_error = Some(format!("Price fetch failed: {e}"));
                }
            }
            state.prices_rx = None;
            state.prices_fetching = false;
        }
    }

    // Request repaints while any background request is in flight
    if state.health_rx.is_some() || state.prices_rx.is_some() {
        ctx.request_repaint();
    }

    style::section_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Inventory Sync:")
                    .color(style::TEXT_MUTED)
                    .size(12.0),
            );

            let te = egui::TextEdit::singleline(&mut state.inventory_sync_url)
                .hint_text("http://127.0.0.1:8080")
                .desired_width(260.0);
            ui.add(te);

            let checking = matches!(state.connection_status, ConnectionStatus::Checking);
            let check_label = if checking { "Checking…" } else { "Check" };
            if style::secondary_button(ui, check_label).clicked() && !checking {
                start_health_check(state);
            }

            // Status indicator
            match &state.connection_status {
                ConnectionStatus::Unchecked => {
                    ui.label(
                        egui::RichText::new("not checked")
                            .color(style::TEXT_MUTED)
                            .size(11.0),
                    );
                }
                ConnectionStatus::Checking => {
                    ui.spinner();
                }
                ConnectionStatus::Connected => {
                    ui.label(
                        egui::RichText::new("connected")
                            .color(egui::Color32::from_rgb(60, 190, 90))
                            .size(11.0),
                    );

                    // Fetch prices button (only when CSV is loaded + connected)
                    if !state.cards.is_empty() {
                        let fetch_label = if state.prices_fetching {
                            "Fetching…"
                        } else {
                            "Fetch Prices"
                        };
                        if style::secondary_button(ui, fetch_label).clicked()
                            && !state.prices_fetching
                        {
                            start_price_fetch(state);
                        }

                        if !state.inventory_prices.is_empty() {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} prices cached",
                                    state.inventory_prices.len()
                                ))
                                .color(style::TEXT_MUTED)
                                .size(11.0),
                            );
                        }
                    }
                }
                ConnectionStatus::Failed(msg) => {
                    ui.label(
                        egui::RichText::new(format!("failed: {msg}"))
                            .color(egui::Color32::from_rgb(220, 60, 60))
                            .size(11.0),
                    );
                }
            }
        });
    });
}

fn start_health_check(state: &mut PricingState) {
    info!(
        "Inventory sync: checking connection to {}",
        state.inventory_sync_url
    );
    state.connection_status = ConnectionStatus::Checking;
    let (tx, rx) = std::sync::mpsc::channel();
    state.health_rx = Some(rx);
    let url = format!(
        "{}/api/health",
        state.inventory_sync_url.trim_end_matches('/')
    );
    std::thread::spawn(move || {
        let result = reqwest::blocking::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send();
        let _ = tx.send(match result {
            Ok(resp) if resp.status().is_success() => Ok(()),
            Ok(resp) => Err(format!("HTTP {}", resp.status())),
            Err(e) => Err(e.to_string()),
        });
    });
}

/// Maximum IDs per request to `/api/latest-prices` (server rejects > 10 000).
const PRICE_FETCH_BATCH_SIZE: usize = 10_000;

fn start_price_fetch(state: &mut PricingState) {
    // Collect unique cardmarket IDs from the loaded CSV
    let ids: Vec<u64> = state
        .cards
        .iter()
        .filter_map(|c| c.cardmarket_id.parse::<u64>().ok())
        .collect::<std::collections::HashSet<u64>>()
        .into_iter()
        .collect();
    if ids.is_empty() {
        return;
    }

    info!(
        "Inventory sync: fetching latest prices for {} products from {}",
        ids.len(),
        state.inventory_sync_url
    );
    state.prices_fetching = true;
    let (tx, rx) = std::sync::mpsc::channel();
    state.prices_rx = Some(rx);
    let url = format!(
        "{}/api/latest-prices",
        state.inventory_sync_url.trim_end_matches('/')
    );
    std::thread::spawn(move || {
        let client = reqwest::blocking::Client::new();
        let mut all_prices = Vec::new();

        for chunk in ids.chunks(PRICE_FETCH_BATCH_SIZE) {
            let body = serde_json::json!({ "ids": chunk });
            let result = client
                .post(&url)
                .json(&body)
                .timeout(std::time::Duration::from_secs(30))
                .send();
            match parse_price_response(result) {
                Ok(prices) => all_prices.extend(prices),
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            }
        }

        let _ = tx.send(Ok(all_prices));
    });
}

/// Parse the JSON response from `/api/latest-prices` into our internal cache format.
fn parse_price_response(
    result: Result<reqwest::blocking::Response, reqwest::Error>,
) -> Result<Vec<(u64, crate::ui::state::CachedLatestPrice)>, String> {
    let resp = result.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    if body.get("success").and_then(|v| v.as_bool()) != Some(true) {
        let msg = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        return Err(msg.to_string());
    }
    let data = body
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or("missing data array")?;
    let mut out = Vec::with_capacity(data.len());
    for entry in data {
        let id = entry
            .get("id_product")
            .and_then(|v| v.as_u64())
            .ok_or("missing id_product")?;
        let f = |key: &str| entry.get(key).and_then(|v| v.as_f64());
        out.push((
            id,
            crate::ui::state::CachedLatestPrice {
                avg: f("avg"),
                low: f("low"),
                trend: f("trend"),
                avg1: f("avg1"),
                avg7: f("avg7"),
                avg30: f("avg30"),
                avg_foil: f("avg_foil"),
                low_foil: f("low_foil"),
                trend_foil: f("trend_foil"),
                avg1_foil: f("avg1_foil"),
                avg7_foil: f("avg7_foil"),
                avg30_foil: f("avg30_foil"),
            },
        ));
    }
    Ok(out)
}
