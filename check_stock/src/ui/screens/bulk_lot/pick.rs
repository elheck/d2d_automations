//! Step 2 — pick the cards: a picture list sorted by shelf location.

use super::widgets::{self, Tone};
use super::{finish, BulkLotState, Confirm};
use crate::ui::style;
use eframe::egui;

const TILE_WIDTH: f32 = 236.0;
const IMAGE_SIZE: egui::Vec2 = egui::vec2(220.0, 307.0);
const TILE_SPACING: f32 = 12.0;
/// Seconds a swapped-in card stays highlighted.
const FLASH_SECONDS: f64 = 2.5;

/// What the user did on a tile.
enum TileAction {
    Toggle(usize),
    Replace(usize),
}

pub(super) fn show(ctx: &egui::Context, state: &mut BulkLotState) {
    show_action_bar(ctx, state);
    egui::CentralPanel::default().show(ctx, |ui| {
        show_progress(ui, state);
        ui.add_space(6.0);
        if let Some(notice) = state.pick_notice.clone() {
            let mut dismiss = false;
            widgets::callout(ui, Tone::Info, &notice, None, |ui| {
                dismiss = ui.small_button("Dismiss").clicked();
            });
            if dismiss {
                state.pick_notice = None;
            }
            ui.add_space(6.0);
        }
        egui::ScrollArea::vertical()
            .id_salt("bulk_lot_pick_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| show_tiles(ui, state));
    });
}

fn show_progress(ui: &mut egui::Ui, state: &mut BulkLotState) {
    let planned = state.planned_copies();
    let picked = state.picked_copies();
    let fraction = if planned > 0 {
        picked as f32 / planned as f32
    } else {
        0.0
    };
    ui.add_space(4.0);
    ui.add(
        egui::ProgressBar::new(fraction)
            .text(format!(
                "{} of {} picked",
                widgets::count(picked),
                widgets::cards(planned)
            ))
            .desired_height(22.0)
            .fill(if picked == planned && planned > 0 {
                style::COLOR_SUCCESS
            } else {
                style::BTN_PRIMARY
            }),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.checkbox(&mut state.hide_picked, "Hide picked cards");
        ui.add_space(12.0);
        if ui.small_button("Mark all picked").clicked() {
            state.set_all_picked(true);
        }
        if ui
            .small_button("Clear ticks")
            .on_hover_text("Untick every card")
            .clicked()
        {
            state.set_all_picked(false);
        }
        let loading = state.images.loading_count();
        if loading > 0 {
            ui.add_space(12.0);
            ui.spinner();
            widgets::hint(ui, &format!("Loading {loading} images…"));
        }
    });
    widgets::hint(
        ui,
        "Sorted by shelf location. Click a card (or “Mark picked”) once it is in your hand. \
         Can't find it, or don't want it in the lot? “Replace” swaps in the next matching card.",
    );
}

fn show_tiles(ui: &mut egui::Ui, state: &mut BulkLotState) {
    let visible: Vec<usize> = (0..state.lines.len())
        .filter(|&i| !(state.hide_picked && state.lines[i].picked))
        .collect();
    if visible.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(if state.lines.is_empty() {
                    "The picking list is empty"
                } else {
                    "All cards picked — finish the lot below"
                })
                .size(16.0)
                .color(style::TEXT_MUTED),
            );
        });
        return;
    }

    let now = ui.input(|i| i.time);
    let columns = (((ui.available_width() + TILE_SPACING) / (TILE_WIDTH + TILE_SPACING)).floor()
        as usize)
        .max(1);
    let mut action = None;
    egui::Grid::new("bulk_lot_pick_grid")
        .num_columns(columns)
        .spacing([TILE_SPACING, TILE_SPACING])
        .show(ui, |ui| {
            for (n, &line) in visible.iter().enumerate() {
                if let Some(a) = tile(ui, state, line, now) {
                    action = Some(a);
                }
                if (n + 1) % columns == 0 {
                    ui.end_row();
                }
            }
        });

    match action {
        Some(TileAction::Toggle(line)) => state.toggle_picked(line),
        Some(TileAction::Replace(line)) => {
            if let Some(&first) = state.replace(line).first() {
                state.flash = Some((first, now + FLASH_SECONDS));
            }
        }
        None => {}
    }
    if state.flash.is_some_and(|(_, until)| now < until) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }
}

fn tile(ui: &mut egui::Ui, state: &mut BulkLotState, line: usize, now: f64) -> Option<TileAction> {
    let entry = state.lines[line];
    let c = &state.candidates[entry.pick.candidate];
    let images = &mut state.images;
    let picked = entry.picked;
    let flashing = state
        .flash
        .is_some_and(|(candidate, until)| candidate == entry.pick.candidate && now < until);
    let (stroke_width, stroke_color) = if picked {
        (2.0_f32, style::COLOR_SUCCESS)
    } else if flashing {
        (2.0_f32, style::ACCENT)
    } else {
        (1.0_f32, style::PANEL_BORDER)
    };

    let mut action = None;
    egui::Frame::new()
        .fill(style::PANEL_BG)
        .stroke(egui::Stroke::new(stroke_width, stroke_color))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            // A grid cell lays its contents out left to right; a tile is a
            // column (image, location, name, details, buttons).
            ui.vertical(|ui| {
                ui.set_width(TILE_WIDTH - 16.0);
                let image = images
                    .show(ui, &c.set_code, &c.cn, IMAGE_SIZE, picked)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(if picked {
                        "Click to untick"
                    } else {
                        "Click when the card is in your hand"
                    });
                if image.clicked() {
                    action = Some(TileAction::Toggle(line));
                }
                ui.add_space(4.0);

                let location = if c.location.is_empty() {
                    egui::RichText::new("No location").color(style::TEXT_MUTED)
                } else {
                    egui::RichText::new(&c.location)
                        .monospace()
                        .color(if picked {
                            style::TEXT_MUTED
                        } else {
                            egui::Color32::from_rgb(140, 190, 255)
                        })
                };
                ui.label(location.size(14.0).strong());

                let name = egui::RichText::new(&c.name).size(14.5).strong();
                ui.add(
                    egui::Label::new(if picked {
                        name.strikethrough().color(style::TEXT_MUTED)
                    } else {
                        name.color(style::TEXT_PRIMARY)
                    })
                    .truncate(),
                )
                .on_hover_text(&c.name);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if entry.pick.copies > 1 {
                        widgets::badge(
                            ui,
                            &format!("TAKE {}", entry.pick.copies),
                            style::COLOR_WARNING,
                        );
                    }
                    if entry.pick.hit {
                        widgets::badge(ui, "HIT", style::COLOR_WARNING);
                    }
                    if c.is_foil {
                        widgets::badge(ui, "FOIL", style::ACCENT);
                    }
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("{} · {}", c.condition, c.language))
                                .color(style::TEXT_MUTED),
                        )
                        .truncate(),
                    );
                });
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!(
                            "{} · {}{}",
                            c.set_name,
                            widgets::euro(c.price),
                            c.age_days
                                .map(|d| format!(" · {d} days"))
                                .unwrap_or_default()
                        ))
                        .size(12.0)
                        .color(style::TEXT_MUTED),
                    )
                    .truncate(),
                );

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let toggle = if picked {
                        ui.add(
                            egui::Button::new(
                                egui::RichText::new("✔ Picked").color(egui::Color32::WHITE),
                            )
                            .fill(style::COLOR_SUCCESS.linear_multiply(0.7))
                            .min_size(egui::vec2(110.0, 26.0)),
                        )
                        .on_hover_text("Click to untick")
                    } else {
                        ui.add(
                            egui::Button::new(
                                egui::RichText::new("Mark picked").color(egui::Color32::WHITE),
                            )
                            .fill(style::BTN_PRIMARY)
                            .min_size(egui::vec2(110.0, 26.0)),
                        )
                    };
                    if toggle.clicked() {
                        action = Some(TileAction::Toggle(line));
                    }
                    let replace = style::secondary_button_enabled(ui, "⟲ Replace", !picked)
                        .on_hover_text(
                            "Can't find it or don't want it? Swap in the next matching card.",
                        )
                        .on_disabled_hover_text("Untick the card first");
                    if replace.clicked() {
                        action = Some(TileAction::Replace(line));
                    }
                });
            });
        });
    action
}

fn show_action_bar(ctx: &egui::Context, state: &mut BulkLotState) {
    let planned = state.planned_copies();
    let picked = state.picked_copies();
    let (mut back, mut finish) = (false, false);
    egui::TopBottomPanel::bottom("bulk_lot_pick_actions")
        .frame(
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(16, 10)),
        )
        .show(ctx, |ui| {
            egui::Sides::new().show(
                ui,
                |ui| {
                    back = style::secondary_button(ui, "←  Back to rules").clicked();
                    ui.add_space(12.0);
                    let unpicked = planned - picked;
                    widgets::hint(
                        ui,
                        &if unpicked > 0 && picked > 0 {
                            format!(
                                "{} picked · {} not picked stay in stock",
                                widgets::cards(picked),
                                widgets::count(unpicked)
                            )
                        } else {
                            format!("{} picked", widgets::cards(picked))
                        },
                    );
                },
                |ui| {
                    finish = style::primary_button_enabled(ui, "Finish lot  →", picked > 0)
                        .on_disabled_hover_text("Tick at least one card as picked")
                        .clicked();
                },
            );
        });
    if back {
        if picked > 0 {
            state.confirm = Some(Confirm::LeavePicking);
        } else {
            state.back_to_define();
        }
    }
    if finish {
        finish::open(state);
    }
}
