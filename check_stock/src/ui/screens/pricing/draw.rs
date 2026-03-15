use super::constants::{
    GRID_COLOR, HEADER_H, NODE_BG, NODE_BORDER, PORT_IN_COLOR, PORT_OUT_COLOR, PORT_R, PORT_ROW_H,
};
use super::geometry::{in_port_pos, out_port_pos};
use crate::ui::state::{GraphNode, NodeKind};
use eframe::egui;

pub(super) fn draw_grid(
    painter: &egui::Painter,
    canvas: egui::Rect,
    offset: egui::Vec2,
    zoom: f32,
) {
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

pub(super) fn draw_bezier(
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

pub(super) fn draw_node_chrome(
    painter: &egui::Painter,
    node: &GraphNode,
    rect: egui::Rect,
    output_count: Option<usize>,
    zoom: f32,
    is_selected: bool,
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

    // Selection highlight (drawn over the body border)
    if is_selected {
        painter.rect(
            rect.expand(2.0),
            egui::CornerRadius::same(cr + 2),
            egui::Color32::TRANSPARENT,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(100, 170, 255)),
            egui::StrokeKind::Outside,
        );
    }

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
