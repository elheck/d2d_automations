#[cfg(test)]
#[path = "pricing_tests.rs"]
mod tests;

use crate::{
    io::read_csv,
    models::Card,
    ui::{
        components::FilePicker,
        state::{
            AppState, ConditionFilter, FoilFilter, GraphNode, LanguageFilter, NodeGraph, NodeId,
            NodeKind, PricingState, RarityFilter, Screen, Wire,
        },
        style,
    },
};
use eframe::egui;
use log::{error, info};
use std::collections::{HashMap, VecDeque};

// ── Visual constants ──────────────────────────────────────────────────────────

const NODE_W: f32 = 168.0;
const HEADER_H: f32 = 26.0;
const PORT_ROW_H: f32 = 22.0;
const PARAM_H: f32 = 30.0;
const BOTTOM_PAD: f32 = 10.0;
const PORT_R: f32 = 6.0;
const PORT_HIT_R: f32 = 10.0;

const WIRE_COLOR: egui::Color32 = egui::Color32::from_rgb(110, 175, 255);
const WIRE_PENDING_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 215, 70);
const CANVAS_BG: egui::Color32 = egui::Color32::from_rgb(20, 24, 36);
const GRID_COLOR: egui::Color32 = egui::Color32::from_rgb(36, 43, 62);
const PORT_IN_COLOR: egui::Color32 = egui::Color32::from_rgb(55, 170, 95);
const PORT_OUT_COLOR: egui::Color32 = egui::Color32::from_rgb(80, 120, 220);
const NODE_BG: egui::Color32 = egui::Color32::from_rgb(28, 34, 50);
const NODE_BORDER: egui::Color32 = egui::Color32::from_rgb(55, 65, 92);

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

            // ── Add-node toolbar (only when CSV loaded) ──────────────────────
            ui.add_enabled_ui(!state.cards.is_empty(), |ui| {
                show_add_toolbar(ui, &mut state.graph);
            });

            ui.add_space(4.0);

            // ── Node editor canvas ───────────────────────────────────────────
            show_canvas(ui, ctx, state);
        });
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

// ── Toolbar ───────────────────────────────────────────────────────────────────

fn show_add_toolbar(ui: &mut egui::Ui, graph: &mut NodeGraph) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new("Filter:")
                .color(style::TEXT_MUTED)
                .size(12.0),
        );
        if style::secondary_button(ui, "▼ Condition").clicked() {
            graph.add_node(
                NodeKind::FilterCondition {
                    condition: ConditionFilter::Any,
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Language").clicked() {
            graph.add_node(
                NodeKind::FilterLanguage {
                    language: LanguageFilter::Any,
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Foil").clicked() {
            graph.add_node(
                NodeKind::FilterFoil {
                    mode: FoilFilter::Any,
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Price Range").clicked() {
            graph.add_node(
                NodeKind::FilterPrice {
                    min: 0.0,
                    max: 999.0,
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Rarity").clicked() {
            graph.add_node(
                NodeKind::FilterRarity {
                    rarity: RarityFilter::Any,
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Name").clicked() {
            graph.add_node(
                NodeKind::FilterName {
                    term: String::new(),
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Set").clicked() {
            graph.add_node(
                NodeKind::FilterSet {
                    term: String::new(),
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Location").clicked() {
            graph.add_node(
                NodeKind::FilterLocation {
                    term: String::new(),
                },
                free_pos(graph),
            );
        }

        ui.add_space(16.0);
        ui.label(
            egui::RichText::new(
                "Right-click to remove  |  Drag header to move  |  Drag output port to wire",
            )
            .color(style::TEXT_MUTED)
            .size(11.0),
        );
    });
}

fn free_pos(graph: &NodeGraph) -> egui::Pos2 {
    let n = graph.nodes.len() as f32;
    egui::pos2(180.0 + (n * 24.0) % 100.0, 80.0 + n * 32.0)
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

    // Evaluate graph to get output card counts for every node
    let counts = evaluate_counts(&state.graph.nodes, &state.graph.wires, &state.cards);

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
            draw_node_chrome(&painter, node, rect, counts.get(&node.id).copied(), zoom);
        }
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

// ── Geometry ──────────────────────────────────────────────────────────────────

fn node_size(kind: &NodeKind) -> egui::Vec2 {
    let port_rows = kind.input_count().max(kind.output_count());
    let h =
        HEADER_H + port_rows as f32 * PORT_ROW_H + kind.param_count() as f32 * PARAM_H + BOTTOM_PAD;
    egui::vec2(NODE_W, h)
}

fn out_port_pos(rect: egui::Rect, idx: usize, zoom: f32) -> egui::Pos2 {
    egui::pos2(
        rect.max.x,
        rect.min.y + HEADER_H * zoom + (idx as f32 + 0.5) * PORT_ROW_H * zoom,
    )
}

fn in_port_pos(rect: egui::Rect, idx: usize, zoom: f32) -> egui::Pos2 {
    egui::pos2(
        rect.min.x,
        rect.min.y + HEADER_H * zoom + (idx as f32 + 0.5) * PORT_ROW_H * zoom,
    )
}

// ── Drawing ───────────────────────────────────────────────────────────────────

fn draw_grid(painter: &egui::Painter, canvas: egui::Rect, offset: egui::Vec2, zoom: f32) {
    let spacing = (32.0_f32 * zoom).max(8.0);
    let ox = offset.x.rem_euclid(spacing);
    let oy = offset.y.rem_euclid(spacing);
    let mut x = canvas.min.x + ox;
    while x < canvas.max.x {
        let mut y = canvas.min.y + oy;
        while y < canvas.max.y {
            painter.circle_filled(egui::pos2(x, y), 1.0, GRID_COLOR);
            y += spacing;
        }
        x += spacing;
    }
}

fn draw_bezier(
    painter: &egui::Painter,
    p0: egui::Pos2,
    p3: egui::Pos2,
    color: egui::Color32,
    width: f32,
) {
    let dx = (p3.x - p0.x).abs().max(80.0) * 0.5;
    let p1 = p0 + egui::vec2(dx, 0.0);
    let p2 = p3 - egui::vec2(dx, 0.0);

    // Approximate bezier with line segments (avoids PathStroke complexity)
    let stroke = egui::Stroke::new(width, color);
    let mut prev = p0;
    for i in 1..=24_usize {
        let t = i as f32 / 24.0;
        let mt = 1.0 - t;
        let pt = egui::pos2(
            mt * mt * mt * p0.x
                + 3.0 * mt * mt * t * p1.x
                + 3.0 * mt * t * t * p2.x
                + t * t * t * p3.x,
            mt * mt * mt * p0.y
                + 3.0 * mt * mt * t * p1.y
                + 3.0 * mt * t * t * p2.y
                + t * t * t * p3.y,
        );
        painter.line_segment([prev, pt], stroke);
        prev = pt;
    }
}

fn draw_node_chrome(
    painter: &egui::Painter,
    node: &GraphNode,
    rect: egui::Rect,
    output_count: Option<usize>,
    zoom: f32,
) {
    let accent = node.kind.accent_color();
    let cr = (6.0 * zoom).min(255.0) as u8;
    let corner = egui::CornerRadius::same(cr);

    // Node body
    painter.rect(
        rect,
        corner,
        NODE_BG,
        egui::Stroke::new(1.5, NODE_BORDER),
        egui::StrokeKind::Inside,
    );

    // Header bar (rounded top corners only)
    let header_rect =
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), HEADER_H * zoom));
    painter.rect(
        header_rect,
        egui::CornerRadius {
            nw: cr,
            ne: cr,
            sw: 0,
            se: 0,
        },
        accent,
        egui::Stroke::NONE,
        egui::StrokeKind::Inside,
    );

    // Title
    painter.text(
        header_rect.center(),
        egui::Align2::CENTER_CENTER,
        node.kind.title(),
        egui::FontId::proportional(13.0 * zoom),
        egui::Color32::WHITE,
    );

    // Input ports (left edge)
    for i in 0..node.kind.input_count() {
        let pos = in_port_pos(rect, i, zoom);
        painter.circle(
            pos,
            PORT_R * zoom,
            PORT_IN_COLOR,
            egui::Stroke::new(1.5, egui::Color32::WHITE),
        );
        painter.text(
            pos + egui::vec2(PORT_R * zoom + 5.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "in",
            egui::FontId::proportional(10.0 * zoom),
            egui::Color32::from_rgb(155, 170, 200),
        );
    }

    // Output ports (right edge) — label shows card count when available
    for i in 0..node.kind.output_count() {
        let pos = out_port_pos(rect, i, zoom);
        painter.circle(
            pos,
            PORT_R * zoom,
            PORT_OUT_COLOR,
            egui::Stroke::new(1.5, egui::Color32::WHITE),
        );
        let (count_text, count_color) = match output_count {
            None => ("—".to_string(), egui::Color32::from_rgb(100, 110, 140)),
            Some(0) => ("0".to_string(), egui::Color32::from_rgb(220, 120, 60)),
            Some(n) => (format!("{n}"), egui::Color32::from_rgb(190, 215, 255)),
        };
        painter.text(
            pos - egui::vec2(PORT_R * zoom + 5.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            &count_text,
            egui::FontId::proportional(11.0 * zoom),
            count_color,
        );
    }

    // Output node: show incoming count prominently in the body (no output port)
    if matches!(node.kind, NodeKind::Output) {
        let body_center = egui::pos2(
            rect.center().x,
            rect.min.y + HEADER_H * zoom + PORT_ROW_H * zoom * 0.5,
        );
        let (text, color) = match output_count {
            None => ("—".to_string(), egui::Color32::from_rgb(100, 110, 140)),
            Some(0) => ("0 cards".to_string(), egui::Color32::from_rgb(220, 120, 60)),
            Some(n) => (format!("{n} cards"), egui::Color32::from_rgb(255, 215, 80)),
        };
        painter.text(
            body_center,
            egui::Align2::CENTER_CENTER,
            &text,
            egui::FontId::proportional(13.0 * zoom),
            color,
        );
    }
}

// ── Parameter widgets ─────────────────────────────────────────────────────────

/// Absolute screen-space rect for one parameter row inside a node.
fn param_row_rect(rect: egui::Rect, port_rows: usize, row: usize, zoom: f32) -> egui::Rect {
    let y = rect.min.y
        + HEADER_H * zoom
        + port_rows as f32 * PORT_ROW_H * zoom
        + 4.0
        + row as f32 * PARAM_H * zoom;
    egui::Rect::from_min_size(
        egui::pos2(rect.min.x + 8.0, y),
        egui::vec2(rect.width() - 16.0, PARAM_H * zoom - 8.0),
    )
}

fn show_node_params(ui: &mut egui::Ui, node: &mut GraphNode, rect: egui::Rect, zoom: f32) {
    let port_rows = node.kind.input_count().max(node.kind.output_count());

    match &mut node.kind {
        NodeKind::FilterCondition { condition } => {
            let r = param_row_rect(rect, port_rows, 0, zoom);
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
                egui::ComboBox::from_id_salt(("fcond", node.id))
                    .selected_text(condition.as_str())
                    .width(r.width())
                    .show_ui(ui, |ui| {
                        for &c in ConditionFilter::all() {
                            ui.selectable_value(condition, c, c.as_str());
                        }
                    });
            });
        }
        NodeKind::FilterLanguage { language } => {
            let r = param_row_rect(rect, port_rows, 0, zoom);
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
                egui::ComboBox::from_id_salt(("flang", node.id))
                    .selected_text(language.as_str())
                    .width(r.width())
                    .show_ui(ui, |ui| {
                        for &l in LanguageFilter::all() {
                            ui.selectable_value(language, l, l.as_str());
                        }
                    });
            });
        }
        NodeKind::FilterFoil { mode } => {
            let r = param_row_rect(rect, port_rows, 0, zoom);
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
                egui::ComboBox::from_id_salt(("ffoil", node.id))
                    .selected_text(mode.as_str())
                    .width(r.width())
                    .show_ui(ui, |ui| {
                        for &m in FoilFilter::all() {
                            ui.selectable_value(mode, m, m.as_str());
                        }
                    });
            });
        }
        NodeKind::FilterPrice { min, max } => {
            ui.put(
                param_row_rect(rect, port_rows, 0, zoom),
                egui::DragValue::new(min)
                    .prefix("≥ ")
                    .suffix(" €")
                    .speed(0.05)
                    .range(0.0..=99999.0),
            );
            ui.put(
                param_row_rect(rect, port_rows, 1, zoom),
                egui::DragValue::new(max)
                    .prefix("≤ ")
                    .suffix(" €")
                    .speed(0.5)
                    .range(0.0..=99999.0),
            );
        }
        NodeKind::FilterRarity { rarity } => {
            let r = param_row_rect(rect, port_rows, 0, zoom);
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
                egui::ComboBox::from_id_salt(("frare", node.id))
                    .selected_text(rarity.as_str())
                    .width(r.width())
                    .show_ui(ui, |ui| {
                        for &rv in RarityFilter::all() {
                            ui.selectable_value(rarity, rv, rv.as_str());
                        }
                    });
            });
        }

        NodeKind::FilterName { term } => {
            ui.put(
                param_row_rect(rect, port_rows, 0, zoom),
                egui::TextEdit::singleline(term).hint_text("name contains…"),
            );
        }
        NodeKind::FilterSet { term } => {
            ui.put(
                param_row_rect(rect, port_rows, 0, zoom),
                egui::TextEdit::singleline(term).hint_text("set name or code…"),
            );
        }
        NodeKind::FilterLocation { term } => {
            ui.put(
                param_row_rect(rect, port_rows, 0, zoom),
                egui::TextEdit::singleline(term).hint_text("location contains…"),
            );
        }

        NodeKind::CsvSource | NodeKind::Output => {}
    }
}

// ── Interaction ───────────────────────────────────────────────────────────────

fn handle_interactions(
    ctx: &egui::Context,
    response: &egui::Response,
    rects: &[(NodeId, egui::Rect)],
    graph: &mut NodeGraph,
    canvas_rect: egui::Rect,
    zoom: f32,
) {
    let mouse_pos = response.hover_pos();
    let pressed = ctx.input(|i| i.pointer.primary_pressed());
    let released = ctx.input(|i| i.pointer.primary_released());
    let right_pressed = ctx.input(|i| i.pointer.secondary_pressed());
    let drag_delta = response.drag_delta();

    // Scroll wheel: zoom centered on cursor
    if response.hovered() {
        let scroll_y = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll_y != 0.0 {
            let factor: f32 = if scroll_y > 0.0 { 1.03 } else { 1.0 / 1.03 };
            let old_zoom = graph.canvas_zoom;
            let new_zoom = (old_zoom * factor).clamp(0.15, 5.0);
            if let Some(cursor) = mouse_pos {
                let origin = canvas_rect.min.to_vec2();
                // Keep the canvas point under the cursor fixed
                let cursor_canvas = (cursor.to_vec2() - origin - graph.canvas_offset) / old_zoom;
                graph.canvas_offset = cursor.to_vec2() - origin - cursor_canvas * new_zoom;
            }
            graph.canvas_zoom = new_zoom;
        }
    }

    // Apply drag delta to the node being dragged (divide by zoom: screen→canvas space)
    if let Some((drag_id, _)) = graph.drag {
        if let Some(node) = graph.node_mut(drag_id) {
            node.pos += drag_delta / zoom;
        }
        if released {
            graph.drag = None;
        }
    } else if graph.pending_wire.is_none() {
        // Pan canvas when dragging on empty space
        graph.canvas_offset += drag_delta;
    }

    // Complete or cancel pending wire on mouse release
    if released {
        if let Some((from_id, from_port)) = graph.pending_wire.take() {
            if let Some(mpos) = mouse_pos {
                // Find which input port the cursor is over
                let mut new_wire: Option<Wire> = None;
                'find: for (node_id, rect) in rects {
                    if *node_id == from_id {
                        continue;
                    }
                    let in_count = graph
                        .nodes
                        .iter()
                        .find(|n| n.id == *node_id)
                        .map(|n| n.kind.input_count())
                        .unwrap_or(0);
                    for p in 0..in_count {
                        if mpos.distance(in_port_pos(*rect, p, zoom)) <= PORT_HIT_R * zoom {
                            new_wire = Some(Wire {
                                from_node: from_id,
                                from_port,
                                to_node: *node_id,
                                to_port: p,
                            });
                            break 'find;
                        }
                    }
                }
                if let Some(wire) = new_wire {
                    // Each input port accepts only one wire
                    graph
                        .wires
                        .retain(|w| !(w.to_node == wire.to_node && w.to_port == wire.to_port));
                    graph.wires.push(wire);
                }
            }
        }
    }

    // Start a new wire drag or node header drag on press
    if pressed && graph.drag.is_none() && graph.pending_wire.is_none() {
        if let Some(mpos) = mouse_pos {
            let mut started_wire = false;

            // Output port check (priority over header drag)
            'outer: for (node_id, rect) in rects {
                let out_count = graph
                    .nodes
                    .iter()
                    .find(|n| n.id == *node_id)
                    .map(|n| n.kind.output_count())
                    .unwrap_or(0);
                for p in 0..out_count {
                    if mpos.distance(out_port_pos(*rect, p, zoom)) <= PORT_HIT_R * zoom {
                        graph.pending_wire = Some((*node_id, p));
                        started_wire = true;
                        break 'outer;
                    }
                }
            }

            // Header drag
            if !started_wire {
                for (node_id, rect) in rects {
                    let header = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(rect.width(), HEADER_H * zoom),
                    );
                    if header.contains(mpos) {
                        graph.drag = Some((*node_id, egui::vec2(0.0, 0.0)));
                        break;
                    }
                }
            }
        }
    }

    // Right-click: delete non-permanent nodes
    if right_pressed {
        if let Some(mpos) = mouse_pos {
            let clicked_id = rects
                .iter()
                .find(|(_, rect)| rect.contains(mpos))
                .map(|(id, _)| *id);
            if let Some(id) = clicked_id {
                let permanent = graph
                    .nodes
                    .iter()
                    .find(|n| n.id == id)
                    .map(|n| matches!(n.kind, NodeKind::CsvSource | NodeKind::Output))
                    .unwrap_or(false);
                if !permanent {
                    graph.remove_node(id);
                }
            }
        }
    }

    // Escape cancels any in-progress interaction
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        graph.pending_wire = None;
        graph.drag = None;
    }
}

// ── Graph evaluation ──────────────────────────────────────────────────────────

/// Returns the output card count for every node, evaluated in topological order.
/// Uses card indices (not clones) for efficiency. Price-modifying nodes do not
/// affect downstream filter counts (acceptable approximation for display).
fn evaluate_counts(
    nodes: &[GraphNode],
    wires: &[Wire],
    all_cards: &[Card],
) -> HashMap<NodeId, usize> {
    if all_cards.is_empty() {
        return HashMap::new();
    }

    // incoming[(to_node, to_port)] = from_node
    let incoming: HashMap<(NodeId, usize), NodeId> = wires
        .iter()
        .map(|w| ((w.to_node, w.to_port), w.from_node))
        .collect();

    // Topological sort (Kahn's algorithm)
    let mut adj: HashMap<NodeId, Vec<NodeId>> = nodes.iter().map(|n| (n.id, vec![])).collect();
    let mut in_deg: HashMap<NodeId, usize> = nodes.iter().map(|n| (n.id, 0)).collect();
    for w in wires {
        adj.entry(w.from_node).or_default().push(w.to_node);
        *in_deg.entry(w.to_node).or_insert(0) += 1;
    }
    let mut queue: VecDeque<NodeId> = in_deg
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&id, _)| id)
        .collect();
    let mut topo: Vec<NodeId> = Vec::with_capacity(nodes.len());
    let mut deg = in_deg.clone();
    while let Some(id) = queue.pop_front() {
        topo.push(id);
        if let Some(neighbors) = adj.get(&id) {
            for &next in neighbors {
                let d = deg.entry(next).or_insert(1);
                *d -= 1;
                if *d == 0 {
                    queue.push_back(next);
                }
            }
        }
    }

    // Evaluate: propagate index sets through the graph
    let all_indices: Vec<usize> = (0..all_cards.len()).collect();
    let mut outputs: HashMap<NodeId, Vec<usize>> = HashMap::new();

    for id in topo {
        let node = match nodes.iter().find(|n| n.id == id) {
            Some(n) => n,
            None => continue,
        };

        let input: Vec<usize> = if node.kind.input_count() == 0 {
            all_indices.clone()
        } else {
            incoming
                .get(&(id, 0))
                .and_then(|&from| outputs.get(&from))
                .cloned()
                .unwrap_or_default()
        };

        let output = filter_indices(&node.kind, input, all_cards);
        outputs.insert(id, output);
    }

    outputs.into_iter().map(|(id, v)| (id, v.len())).collect()
}

/// Apply a node's filtering logic to a set of card indices.
/// Price-transform nodes pass indices unchanged (counts are unaffected by price edits).
fn filter_indices(kind: &NodeKind, indices: Vec<usize>, cards: &[Card]) -> Vec<usize> {
    match kind {
        NodeKind::CsvSource | NodeKind::Output => indices,

        NodeKind::FilterCondition { condition } => {
            if matches!(condition, ConditionFilter::Any) {
                return indices;
            }
            let target = condition.as_str();
            indices
                .into_iter()
                .filter(|&i| cards[i].condition.eq_ignore_ascii_case(target))
                .collect()
        }

        NodeKind::FilterLanguage { language } => {
            if matches!(language, LanguageFilter::Any) {
                return indices;
            }
            let t = language.as_str();
            indices
                .into_iter()
                .filter(|&i| cards[i].language.eq_ignore_ascii_case(t))
                .collect()
        }

        NodeKind::FilterFoil { mode } => match mode {
            FoilFilter::Any => indices,
            FoilFilter::FoilOnly => indices
                .into_iter()
                .filter(|&i| cards[i].is_foil_card())
                .collect(),
            FoilFilter::NonFoilOnly => indices
                .into_iter()
                .filter(|&i| !cards[i].is_foil_card())
                .collect(),
        },

        NodeKind::FilterPrice { min, max } => indices
            .into_iter()
            .filter(|&i| {
                let p = cards[i].price.parse::<f64>().unwrap_or(0.0);
                p >= *min && p <= *max
            })
            .collect(),

        NodeKind::FilterRarity { rarity } => {
            if matches!(rarity, RarityFilter::Any) {
                return indices;
            }
            let t = rarity.as_str();
            indices
                .into_iter()
                .filter(|&i| cards[i].rarity.eq_ignore_ascii_case(t))
                .collect()
        }

        NodeKind::FilterName { term } => {
            if term.is_empty() {
                return indices;
            }
            let t = term.to_lowercase();
            indices
                .into_iter()
                .filter(|&i| cards[i].name.to_lowercase().contains(&t))
                .collect()
        }

        NodeKind::FilterSet { term } => {
            if term.is_empty() {
                return indices;
            }
            let t = term.to_lowercase();
            indices
                .into_iter()
                .filter(|&i| {
                    cards[i].set.to_lowercase().contains(&t)
                        || cards[i].set_code.to_lowercase().contains(&t)
                })
                .collect()
        }

        NodeKind::FilterLocation { term } => {
            if term.is_empty() {
                return indices;
            }
            let t = term.to_lowercase();
            indices
                .into_iter()
                .filter(|&i| {
                    cards[i]
                        .location
                        .as_deref()
                        .map(|l| l.to_lowercase().contains(&t))
                        .unwrap_or(false)
                })
                .collect()
        }
    }
}
