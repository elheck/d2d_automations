use eframe::egui;

pub(super) const NODE_W: f32 = 168.0;
pub(super) const HEADER_H: f32 = 26.0;
pub(super) const PORT_ROW_H: f32 = 22.0;
pub(super) const PARAM_H: f32 = 30.0;
pub(super) const BOTTOM_PAD: f32 = 10.0;
pub(super) const PORT_R: f32 = 6.0;
pub(super) const PORT_HIT_R: f32 = 10.0;

pub(super) const WIRE_COLOR: egui::Color32 = egui::Color32::from_rgb(110, 175, 255);
pub(super) const WIRE_PENDING_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 215, 70);
pub(super) const CANVAS_BG: egui::Color32 = egui::Color32::from_rgb(20, 24, 36);
pub(super) const GRID_COLOR: egui::Color32 = egui::Color32::from_rgb(36, 43, 62);
pub(super) const PORT_IN_COLOR: egui::Color32 = egui::Color32::from_rgb(55, 170, 95);
pub(super) const PORT_OUT_COLOR: egui::Color32 = egui::Color32::from_rgb(80, 120, 220);
pub(super) const NODE_BG: egui::Color32 = egui::Color32::from_rgb(28, 34, 50);
pub(super) const NODE_BORDER: egui::Color32 = egui::Color32::from_rgb(55, 65, 92);
