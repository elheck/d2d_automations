//! Shared visual style helpers.
//!
//! Pure presentation utilities — no business logic, no state.
use eframe::egui;

// ── Palette ──────────────────────────────────────────────────────────────────
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(80, 120, 220);
pub const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(220, 220, 230);
pub const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(140, 148, 168);
pub const BTN_PRIMARY: egui::Color32 = egui::Color32::from_rgb(55, 95, 180);
pub const BTN_SECONDARY: egui::Color32 = egui::Color32::from_rgb(42, 50, 72);
pub const PANEL_BG: egui::Color32 = egui::Color32::from_rgb(30, 36, 52);
pub const PANEL_BORDER: egui::Color32 = egui::Color32::from_rgb(52, 62, 88);
pub const COLOR_SUCCESS: egui::Color32 = egui::Color32::from_rgb(75, 175, 115);
pub const COLOR_ERROR: egui::Color32 = egui::Color32::from_rgb(210, 75, 75);

// ── Navigation ───────────────────────────────────────────────────────────────

/// Frameless back-navigation text button.
pub fn back_button(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add(
        egui::Button::new(
            egui::RichText::new(format!("← {label}"))
                .color(TEXT_MUTED)
                .size(13.0),
        )
        .frame(false),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
}

// ── Headings ─────────────────────────────────────────────────────────────────

/// Large screen title followed by a thin accent underline.
pub fn screen_heading(ui: &mut egui::Ui, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .size(22.0)
            .strong()
            .color(TEXT_PRIMARY),
    );
    ui.add_space(4.0);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.5), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, ACCENT.linear_multiply(0.45));
    ui.add_space(10.0);
}

// ── Buttons ──────────────────────────────────────────────────────────────────

/// Filled primary action button (always enabled).
pub fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .color(egui::Color32::WHITE)
                .size(14.0),
        )
        .fill(BTN_PRIMARY)
        .min_size(egui::vec2(120.0, 28.0)),
    )
}

/// Primary action button that can be conditionally disabled.
pub fn primary_button_enabled(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(
            egui::RichText::new(label)
                .color(egui::Color32::WHITE)
                .size(14.0),
        )
        .fill(BTN_PRIMARY)
        .min_size(egui::vec2(80.0, 28.0)),
    )
}

/// Subtler secondary action button.
pub fn secondary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(TEXT_PRIMARY).size(13.0))
            .fill(BTN_SECONDARY)
            .min_size(egui::vec2(80.0, 26.0)),
    )
}

/// Secondary button that can be conditionally disabled.
pub fn secondary_button_enabled(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(egui::RichText::new(label).color(TEXT_PRIMARY).size(13.0))
            .fill(BTN_SECONDARY)
            .min_size(egui::vec2(80.0, 26.0)),
    )
}

// ── Frames ───────────────────────────────────────────────────────────────────

/// Dark card frame for grouping related controls.
pub fn section_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL_BG)
        .stroke(egui::Stroke::new(1.0, PANEL_BORDER))
        .inner_margin(egui::Margin::same(12))
        .corner_radius(egui::CornerRadius::same(8))
}

// ── Status labels ────────────────────────────────────────────────────────────

pub fn status_ok(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(COLOR_SUCCESS).size(13.0));
}

pub fn status_loading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(TEXT_MUTED).size(13.0));
}

pub fn status_error(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(COLOR_ERROR).size(13.0));
}
