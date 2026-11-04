//! Small visual building blocks of the Bulk Lots screen.

use crate::ui::style;
use eframe::egui;

/// Severity of a [`callout`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tone {
    Info,
    Warning,
    Error,
}

impl Tone {
    fn color(self) -> egui::Color32 {
        match self {
            Tone::Info => style::ACCENT,
            Tone::Warning => style::COLOR_WARNING,
            Tone::Error => style::COLOR_ERROR,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Tone::Info => "ℹ",
            Tone::Warning => "⚠",
            Tone::Error => "✖",
        }
    }
}

/// Tinted message box with an icon, a bold title and optional body text.
/// `extra` renders controls (buttons, a progress bar) under the text.
pub(super) fn callout(
    ui: &mut egui::Ui,
    tone: Tone,
    title: &str,
    body: Option<&str>,
    extra: impl FnOnce(&mut egui::Ui),
) {
    let color = tone.color();
    egui::Frame::new()
        .fill(color.linear_multiply(0.12))
        .stroke(egui::Stroke::new(1.0_f32, color.linear_multiply(0.6)))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .corner_radius(egui::CornerRadius::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.label(egui::RichText::new(tone.icon()).color(color).size(15.0));
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(title)
                            .strong()
                            .color(style::TEXT_PRIMARY),
                    );
                    if let Some(body) = body {
                        ui.label(egui::RichText::new(body).color(style::TEXT_MUTED));
                    }
                    extra(ui);
                });
            });
        });
}

/// Key figure: caption, large value, optional muted line underneath.
pub(super) fn stat_tile(
    ui: &mut egui::Ui,
    caption: &str,
    value: &str,
    sub: &str,
    value_color: egui::Color32,
) -> egui::Response {
    style::section_frame()
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            // Tiles sit in wrapping rows; stack caption, value and note.
            ui.vertical(|ui| {
                ui.set_min_width(150.0);
                ui.label(
                    egui::RichText::new(caption.to_uppercase())
                        .size(10.5)
                        .color(style::TEXT_MUTED),
                );
                ui.label(
                    egui::RichText::new(value)
                        .size(20.0)
                        .strong()
                        .color(value_color),
                );
                ui.label(egui::RichText::new(sub).size(11.0).color(style::TEXT_MUTED));
            });
        })
        .response
}

/// Bold title of a settings group, with breathing room above it.
pub(super) fn group_title(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .size(11.0)
            .strong()
            .color(style::ACCENT.linear_multiply(1.4)),
    );
    ui.add_space(4.0);
}

/// Muted explanatory text.
pub(super) fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(11.5)
            .color(style::TEXT_MUTED),
    );
}

/// Label column of a settings row: fixed width so the inputs line up.
pub(super) fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.add_sized(
        [118.0, 20.0],
        egui::Label::new(egui::RichText::new(text).color(style::TEXT_PRIMARY)),
    );
}

/// Toggle chip: a selectable pill; returns true when clicked.
pub(super) fn chip(ui: &mut egui::Ui, selected: &mut bool, text: &str) -> egui::Response {
    let response = ui.selectable_label(*selected, egui::RichText::new(text).size(12.5));
    if response.clicked() {
        *selected = !*selected;
    }
    response
}

/// Small coloured pill, e.g. "HIT" or "FOIL".
pub(super) fn badge(ui: &mut egui::Ui, text: &str, color: egui::Color32) -> egui::Response {
    egui::Frame::new()
        .fill(color.linear_multiply(0.2))
        .stroke(egui::Stroke::new(1.0_f32, color.linear_multiply(0.8)))
        .inner_margin(egui::Margin::symmetric(5, 1))
        .corner_radius(egui::CornerRadius::same(4))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(10.0).strong().color(color));
        })
        .response
}

/// Step indicator: "① Choose cards  ›  ② Pick  ›  ③ Done".
pub(super) fn stepper(ui: &mut egui::Ui, steps: &[&str], current: usize) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (i, step) in steps.iter().enumerate() {
            if i > 0 {
                ui.label(egui::RichText::new("›").color(style::TEXT_MUTED));
            }
            let (marker, color) = if i < current {
                ("✔".to_string(), style::COLOR_SUCCESS)
            } else if i == current {
                (format!("{}", i + 1), style::TEXT_PRIMARY)
            } else {
                (format!("{}", i + 1), style::TEXT_MUTED)
            };
            let circle = if i == current {
                style::ACCENT
            } else {
                style::PANEL_BORDER
            };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 10.0, circle);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                marker,
                egui::FontId::proportional(11.0),
                if i == current {
                    egui::Color32::WHITE
                } else {
                    color
                },
            );
            let text = egui::RichText::new(*step).color(color);
            ui.label(if i == current { text.strong() } else { text });
        }
    });
}

/// Pill-style tab bar; sets `current` when a tab is clicked.
pub(super) fn tabs<T: PartialEq + Copy>(ui: &mut egui::Ui, current: &mut T, items: &[(T, String)]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (value, label) in items {
            let selected = *current == *value;
            let text = egui::RichText::new(label).size(14.0);
            let button = egui::Button::new(if selected { text.strong() } else { text })
                .fill(if selected {
                    style::BTN_PRIMARY
                } else {
                    egui::Color32::TRANSPARENT
                })
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    if selected {
                        style::BTN_PRIMARY
                    } else {
                        style::PANEL_BORDER
                    },
                ))
                .corner_radius(egui::CornerRadius::same(14))
                .min_size(egui::vec2(96.0, 28.0));
            if ui.add(button).clicked() {
                *current = *value;
            }
        }
    });
}

/// Money with two decimals: `€1.00`.
pub(super) fn euro(v: f64) -> String {
    format!("€{v:.2}")
}

/// A per-card share of a lot price: two decimals, more when needed so a
/// share below a cent stays readable (`€0.01`, `€0.0125`, `€0.0033`).
pub(super) fn share(v: f64) -> String {
    let precise = format!("{v:.4}");
    let (whole, decimals) = precise.split_once('.').unwrap_or((&precise, ""));
    let mut decimals = decimals.trim_end_matches('0').to_string();
    while decimals.len() < 2 {
        decimals.push('0');
    }
    format!("€{whole}.{decimals}")
}

/// Whole number with thousands separators: `14,435`.
pub(super) fn count(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// `1 card` / `2 cards`.
pub(super) fn cards(n: i64) -> String {
    if n == 1 {
        "1 card".to_string()
    } else {
        format!("{} cards", count(n))
    }
}
