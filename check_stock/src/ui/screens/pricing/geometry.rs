use super::constants::{BOTTOM_PAD, HEADER_H, NODE_W, PORT_ROW_H};
use crate::ui::state::NodeKind;
use eframe::egui;

pub(super) fn node_size(kind: &NodeKind) -> egui::Vec2 {
    use super::constants::PARAM_H;
    let port_rows = kind.input_count().max(kind.output_count());
    let h =
        HEADER_H + port_rows as f32 * PORT_ROW_H + kind.param_count() as f32 * PARAM_H + BOTTOM_PAD;
    egui::vec2(NODE_W, h)
}

pub(super) fn out_port_pos(rect: egui::Rect, idx: usize, zoom: f32) -> egui::Pos2 {
    egui::pos2(
        rect.max.x,
        rect.min.y + HEADER_H * zoom + (idx as f32 + 0.5) * PORT_ROW_H * zoom,
    )
}

pub(super) fn in_port_pos(rect: egui::Rect, idx: usize, zoom: f32) -> egui::Pos2 {
    egui::pos2(
        rect.min.x,
        rect.min.y + HEADER_H * zoom + (idx as f32 + 0.5) * PORT_ROW_H * zoom,
    )
}
