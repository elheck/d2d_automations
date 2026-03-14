use crate::{
    io::read_csv,
    ui::{
        components::FilePicker,
        state::{AppState, GraphNode, NodeGraph, NodeId, NodeKind, PricingState, Screen, Wire},
        style,
    },
};
use eframe::egui;
use log::{error, info};

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
                if (style::primary_button(ui, "Load CSV").clicked() || browsed)
                    && !state.csv_path.is_empty()
                {
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
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Add node:")
                .color(style::TEXT_MUTED)
                .size(12.0),
        );
        if style::secondary_button(ui, "× Multiply").clicked() {
            graph.add_node(NodeKind::PriceMultiply { factor: 1.0 }, free_pos(graph));
        }
        if style::secondary_button(ui, "↑ Floor").clicked() {
            graph.add_node(NodeKind::PriceFloor { min: 0.25 }, free_pos(graph));
        }
        if style::secondary_button(ui, "↓ Cap").clicked() {
            graph.add_node(NodeKind::PriceCap { max: 100.0 }, free_pos(graph));
        }
        if style::secondary_button(ui, "○ Round").clicked() {
            graph.add_node(NodeKind::PriceRound { step: 0.5 }, free_pos(graph));
        }
        ui.add_space(16.0);
        ui.label(
            egui::RichText::new(
                "Right-click node to remove  |  Drag header to move  |  Drag output port to wire",
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

    // Background + dot grid
    painter.rect_filled(canvas_rect, egui::CornerRadius::ZERO, CANVAS_BG);
    draw_grid(&painter, canvas_rect, state.graph.canvas_offset);

    // Clip everything to canvas bounds
    painter.set_clip_rect(canvas_rect);

    // Precompute node screen-space rects (canvas pos → screen pos)
    let rects: Vec<(NodeId, egui::Rect)> = state
        .graph
        .nodes
        .iter()
        .map(|n| {
            let sp = canvas_rect.min + n.pos.to_vec2() + state.graph.canvas_offset;
            (n.id, egui::Rect::from_min_size(sp, node_size(&n.kind)))
        })
        .collect();

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
                out_port_pos(fr, wire.from_port),
                in_port_pos(tr, wire.to_port),
                WIRE_COLOR,
                2.0,
            );
        }
    }

    // Draw pending wire following the cursor
    if let Some((from_id, from_port)) = state.graph.pending_wire {
        if let Some(fr) = rects.iter().find(|(id, _)| *id == from_id).map(|(_, r)| *r) {
            let start = out_port_pos(fr, from_port);
            let end = canvas_response.hover_pos().unwrap_or(start);
            draw_bezier(&painter, start, end, WIRE_PENDING_COLOR, 1.5);
            ctx.request_repaint();
        }
    }

    // Draw node chrome (background, header, port circles, labels)
    for node in &state.graph.nodes {
        if let Some(rect) = rects.iter().find(|(id, _)| *id == node.id).map(|(_, r)| *r) {
            draw_node_chrome(&painter, node, rect);
        }
    }

    // Parameter widgets sit inside node bodies (ui.put uses screen-space rects)
    for node in &mut state.graph.nodes {
        if let Some(rect) = rects.iter().find(|(id, _)| *id == node.id).map(|(_, r)| *r) {
            show_node_params(ui, node, rect);
        }
    }

    handle_interactions(ctx, &canvas_response, &rects, &mut state.graph);
}

// ── Geometry ──────────────────────────────────────────────────────────────────

fn node_size(kind: &NodeKind) -> egui::Vec2 {
    let port_rows = kind.input_count().max(kind.output_count());
    let h =
        HEADER_H + port_rows as f32 * PORT_ROW_H + kind.param_count() as f32 * PARAM_H + BOTTOM_PAD;
    egui::vec2(NODE_W, h)
}

fn out_port_pos(rect: egui::Rect, idx: usize) -> egui::Pos2 {
    egui::pos2(
        rect.max.x,
        rect.min.y + HEADER_H + (idx as f32 + 0.5) * PORT_ROW_H,
    )
}

fn in_port_pos(rect: egui::Rect, idx: usize) -> egui::Pos2 {
    egui::pos2(
        rect.min.x,
        rect.min.y + HEADER_H + (idx as f32 + 0.5) * PORT_ROW_H,
    )
}

// ── Drawing ───────────────────────────────────────────────────────────────────

fn draw_grid(painter: &egui::Painter, canvas: egui::Rect, offset: egui::Vec2) {
    let spacing = 32.0_f32;
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

fn draw_node_chrome(painter: &egui::Painter, node: &GraphNode, rect: egui::Rect) {
    let accent = node.kind.accent_color();

    // Node body
    painter.rect(
        rect,
        egui::CornerRadius::same(6),
        NODE_BG,
        egui::Stroke::new(1.5, NODE_BORDER),
        egui::StrokeKind::Inside,
    );

    // Header bar (rounded top corners only)
    let header_rect = egui::Rect::from_min_size(rect.min, egui::vec2(NODE_W, HEADER_H));
    painter.rect(
        header_rect,
        egui::CornerRadius {
            nw: 6,
            ne: 6,
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
        egui::FontId::proportional(13.0),
        egui::Color32::WHITE,
    );

    // Input ports (left edge)
    for i in 0..node.kind.input_count() {
        let pos = in_port_pos(rect, i);
        painter.circle(
            pos,
            PORT_R,
            PORT_IN_COLOR,
            egui::Stroke::new(1.5, egui::Color32::WHITE),
        );
        painter.text(
            pos + egui::vec2(PORT_R + 5.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "in",
            egui::FontId::proportional(10.0),
            egui::Color32::from_rgb(155, 170, 200),
        );
    }

    // Output ports (right edge)
    for i in 0..node.kind.output_count() {
        let pos = out_port_pos(rect, i);
        painter.circle(
            pos,
            PORT_R,
            PORT_OUT_COLOR,
            egui::Stroke::new(1.5, egui::Color32::WHITE),
        );
        painter.text(
            pos - egui::vec2(PORT_R + 5.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            "out",
            egui::FontId::proportional(10.0),
            egui::Color32::from_rgb(155, 170, 200),
        );
    }
}

// ── Parameter widgets ─────────────────────────────────────────────────────────

fn show_node_params(ui: &mut egui::Ui, node: &mut GraphNode, rect: egui::Rect) {
    let port_rows = node.kind.input_count().max(node.kind.output_count());
    let param_top = rect.min.y + HEADER_H + port_rows as f32 * PORT_ROW_H + 4.0;
    let param_rect = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + 8.0, param_top),
        egui::vec2(NODE_W - 16.0, PARAM_H - 8.0),
    );

    match &mut node.kind {
        NodeKind::PriceMultiply { factor } => {
            ui.put(
                param_rect,
                egui::DragValue::new(factor)
                    .prefix("× ")
                    .speed(0.01)
                    .range(0.01..=100.0),
            );
        }
        NodeKind::PriceFloor { min } => {
            ui.put(
                param_rect,
                egui::DragValue::new(min)
                    .prefix("min ")
                    .suffix(" €")
                    .speed(0.05)
                    .range(0.0..=9999.0),
            );
        }
        NodeKind::PriceCap { max } => {
            ui.put(
                param_rect,
                egui::DragValue::new(max)
                    .prefix("max ")
                    .suffix(" €")
                    .speed(0.5)
                    .range(0.01..=99999.0),
            );
        }
        NodeKind::PriceRound { step } => {
            ui.put(
                param_rect,
                egui::DragValue::new(step)
                    .prefix("step ")
                    .suffix(" €")
                    .speed(0.01)
                    .range(0.01..=100.0),
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
) {
    let mouse_pos = response.hover_pos();
    let pressed = ctx.input(|i| i.pointer.primary_pressed());
    let released = ctx.input(|i| i.pointer.primary_released());
    let right_pressed = ctx.input(|i| i.pointer.secondary_pressed());
    let drag_delta = response.drag_delta();

    // Apply drag delta to the node being dragged
    if let Some((drag_id, _)) = graph.drag {
        if let Some(node) = graph.node_mut(drag_id) {
            node.pos += drag_delta;
        }
        if released {
            graph.drag = None;
        }
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
                        if mpos.distance(in_port_pos(*rect, p)) <= PORT_HIT_R {
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
                    if mpos.distance(out_port_pos(*rect, p)) <= PORT_HIT_R {
                        graph.pending_wire = Some((*node_id, p));
                        started_wire = true;
                        break 'outer;
                    }
                }
            }

            // Header drag
            if !started_wire {
                for (node_id, rect) in rects {
                    let header = egui::Rect::from_min_size(rect.min, egui::vec2(NODE_W, HEADER_H));
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
