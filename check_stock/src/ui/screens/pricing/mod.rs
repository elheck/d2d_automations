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
        components::FilePicker,
        state::{AppState, NodeId, NodeKind, PricingState, Screen},
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

    // Evaluate graph once — derive per-node counts and cache output-node indices
    let mut all_outputs = evaluate_all(&state.graph.nodes, &state.graph.wires, &state.cards);
    let counts: HashMap<NodeId, usize> = all_outputs.iter().map(|(&id, v)| (id, v.len())).collect();
    // Cache the output node's card indices for the preview window (zero-cost when preview is closed)
    let output_id = state
        .graph
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Output))
        .map(|n| n.id);
    state.cached_output = output_id
        .and_then(|id| all_outputs.remove(&id))
        .unwrap_or_default();
    // Apply the current sort in-place so the preview window is always ready
    if let Some(col) = state.preview_sort_col {
        sort_preview(
            &mut state.cached_output,
            &state.cards,
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
