//! Step 3 — finish: save the TCG Powertools CSV, record the lot, show what
//! happens next.

use super::widgets::{self, Tone};
use super::{BulkLotState, DoneInfo, FinishDialog, Tab};
use crate::bulk_lot::{export_file_name, powertools_csv};
use crate::inventory_db::{self, per_card_share, MAX_LOT_NAME_CHARS, MAX_LOT_PRICE_EUR};
use crate::ui::state::AppState;
use crate::ui::style;
use chrono::NaiveDate;
use eframe::egui;

/// Opens the finish dialog with the name and price from the rules.
pub(super) fn open(state: &mut BulkLotState) {
    state.finish = Some(FinishDialog {
        name: state.name.clone(),
        price: state.price,
        error: None,
        saved_csv: None,
    });
}

/// Why the lot cannot be finished yet, if anything stops it.
fn blocker(dialog: &FinishDialog, guard_pending: bool) -> Option<&'static str> {
    let name = dialog.name.trim();
    if name.is_empty() {
        Some("Give the lot a name")
    } else if name.chars().count() > MAX_LOT_NAME_CHARS {
        Some("The name is too long")
    } else if guard_pending {
        Some("Resolve the pending inventory import first")
    } else {
        None
    }
}

enum Decision {
    Cancel,
    Confirm,
}

pub(super) fn show_dialog(
    ctx: &egui::Context,
    app_state: &mut AppState,
    state: &mut BulkLotState,
    today: NaiveDate,
) {
    if state.finish.is_none() {
        return;
    }
    let summary = state.picked_summary();
    let planned = state.planned_copies();
    let guard_pending = app_state.sync_guard.is_some();
    let Some(dialog) = state.finish.as_mut() else {
        return;
    };

    let mut decision = None;
    let modal = egui::Modal::new(egui::Id::new("bulk_lot_finish")).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.label(
            egui::RichText::new("Finish lot")
                .size(18.0)
                .strong()
                .color(style::TEXT_PRIMARY),
        );
        widgets::hint(ui, "Check the details. Saving the CSV records the lot.");
        ui.add_space(12.0);

        let locked = dialog.saved_csv.is_some();
        egui::Grid::new("bulk_lot_finish_fields")
            .num_columns(2)
            .spacing([14.0, 8.0])
            .show(ui, |ui| {
                ui.label("Lot name");
                ui.add_enabled(
                    !locked,
                    egui::TextEdit::singleline(&mut dialog.name)
                        .desired_width(300.0)
                        .char_limit(MAX_LOT_NAME_CHARS),
                );
                ui.end_row();
                ui.label("Lot price");
                ui.add_enabled(
                    !locked,
                    egui::DragValue::new(&mut dialog.price)
                        .range(0.0..=MAX_LOT_PRICE_EUR)
                        .speed(0.05)
                        .fixed_decimals(2)
                        .prefix("€"),
                );
                ui.end_row();
            });
        ui.add_space(12.0);

        style::section_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("bulk_lot_finish_summary")
                .num_columns(2)
                .spacing([18.0, 6.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Cards in the lot").color(style::TEXT_MUTED));
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(widgets::count(summary.copies)).strong());
                        if summary.copies < planned {
                            widgets::hint(
                                ui,
                                &format!(
                                    "({} not picked stay in stock)",
                                    widgets::count(planned - summary.copies)
                                ),
                            );
                        }
                    });
                    ui.end_row();
                    ui.label(egui::RichText::new("Single value").color(style::TEXT_MUTED));
                    ui.label(format!(
                        "{}  — what these cards are listed at",
                        widgets::euro(summary.listed_value)
                    ));
                    ui.end_row();
                    ui.label(egui::RichText::new("Revenue booked today").color(style::TEXT_MUTED));
                    ui.label(
                        egui::RichText::new(format!(
                            "{}  ({} per card)",
                            widgets::euro(dialog.price),
                            widgets::share(per_card_share(dialog.price, summary.copies))
                        ))
                        .strong()
                        .color(style::COLOR_SUCCESS),
                    );
                    ui.end_row();
                    ui.label(
                        egui::RichText::new("Purchase lots credited").color(style::TEXT_MUTED),
                    );
                    let lots: Vec<String> = summary
                        .purchase_lots
                        .iter()
                        .map(|(lot, n)| {
                            let lot = if lot.is_empty() {
                                "no lot"
                            } else {
                                lot.as_str()
                            };
                            format!("{lot} ×{n}")
                        })
                        .collect();
                    ui.add(egui::Label::new(lots.join(" · ")).wrap());
                    ui.end_row();
                });
        });
        ui.add_space(12.0);

        ui.label(
            egui::RichText::new("What happens")
                .strong()
                .color(style::TEXT_PRIMARY),
        );
        ui.add_space(2.0);
        for (n, text) in [
            format!(
                "You choose where to save the TCG Powertools CSV. It removes these {} from \
                 your Cardmarket stock.",
                widgets::cards(summary.copies)
            ),
            "The lot price is booked as today's revenue and split across the cards' purchase \
             lots. The cards never count as single sales."
                .to_string(),
            "Import the CSV in TCG Powertools. The next inventory export you load recognises \
             the removal automatically."
                .to_string(),
        ]
        .iter()
        .enumerate()
        {
            ui.horizontal_top(|ui| {
                ui.label(egui::RichText::new(format!("{}.", n + 1)).color(style::ACCENT));
                ui.add(egui::Label::new(egui::RichText::new(text).color(style::TEXT_MUTED)).wrap());
            });
        }

        if let Some(path) = &dialog.saved_csv {
            ui.add_space(10.0);
            widgets::callout(
                ui,
                Tone::Warning,
                "The CSV is saved, but the lot is not recorded yet",
                Some(&format!(
                    "{}\nDon't import it into TCG Powertools until the lot is recorded — \
                     otherwise the next inventory import counts the cards as single sales.",
                    path.display()
                )),
                |_| {},
            );
        }
        if let Some(err) = &dialog.error {
            ui.add_space(8.0);
            widgets::callout(ui, Tone::Error, "Something went wrong", Some(err), |_| {});
        }

        ui.add_space(14.0);
        let block = blocker(dialog, guard_pending);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if locked {
                "Retry recording"
            } else {
                "Save CSV & record lot…"
            };
            let confirm = style::primary_button_enabled(ui, label, block.is_none());
            let confirm = match block {
                Some(reason) => confirm.on_disabled_hover_text(reason),
                None => confirm,
            };
            if confirm.clicked() {
                decision = Some(Decision::Confirm);
            }
            let cancel = if locked {
                "Close without recording"
            } else {
                "Cancel"
            };
            if style::secondary_button(ui, cancel).clicked() {
                decision = Some(Decision::Cancel);
            }
        });
    });
    // Escape or a click outside closes the dialog — but not once a CSV
    // exists on disk without a recorded lot; then the user must decide.
    if decision.is_none() && modal.should_close() && dialog.saved_csv.is_none() {
        decision = Some(Decision::Cancel);
    }

    match decision {
        Some(Decision::Cancel) => state.finish = None,
        Some(Decision::Confirm) => perform(app_state, state, today),
        None => {}
    }
}

/// Saves the CSV (once) and records the lot.
fn perform(app_state: &mut AppState, state: &mut BulkLotState, today: NaiveDate) {
    let lines = state.picked_lines();
    let copies: i64 = lines.iter().map(|(_, n)| n).sum();
    let category = state
        .source
        .as_ref()
        .map(|s| s.category.clone())
        .unwrap_or_default();
    let Some(dialog) = state.finish.as_mut() else {
        return;
    };
    if blocker(dialog, app_state.sync_guard.is_some()).is_some() || lines.is_empty() {
        return;
    }
    let name = dialog.name.trim().to_string();
    dialog.error = None;

    let path = match &dialog.saved_csv {
        Some(path) => path.clone(),
        None => {
            let file_name = export_file_name(&name, &today.format("%Y-%m-%d").to_string());
            let Some(path) = rfd::FileDialog::new()
                .set_title("Save TCG Powertools CSV")
                .set_file_name(&file_name)
                .add_filter("CSV", &["csv"])
                .save_file()
            else {
                // Cancelled: nothing written, nothing recorded.
                return;
            };
            if let Err(e) = std::fs::write(&path, powertools_csv(&lines)) {
                dialog.error = Some(format!("Could not save the CSV to {}: {e}", path.display()));
                return;
            }
            log::info!("Bulk lot CSV saved to {}", path.display());
            dialog.saved_csv = Some(path.clone());
            path
        }
    };

    match inventory_db::record_bulk_lot(&name, dialog.price, &lines, &category) {
        Ok(_) => {
            let price = dialog.price;
            state.name = name.clone();
            app_state.mark_db_changed();
            state.finish_recorded(DoneInfo {
                name,
                copies,
                price,
                csv: path,
            });
        }
        Err(e) => {
            dialog.error = Some(format!("The lot could not be recorded: {e}"));
        }
    }
}

/// The result page after a lot was recorded.
pub(super) fn show_done(ctx: &egui::Context, state: &mut BulkLotState) {
    egui::CentralPanel::default().show(ctx, |ui| {
        let Some(done) = &state.done else {
            return;
        };
        let mut another = false;
        let mut history = false;
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(640.0);
                style::section_frame()
                    .inner_margin(egui::Margin::same(24))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
                            ui.painter()
                                .circle_filled(rect.center(), 17.0, style::COLOR_SUCCESS);
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "✔",
                                egui::FontId::proportional(18.0),
                                egui::Color32::WHITE,
                            );
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new("Lot recorded")
                                        .size(19.0)
                                        .strong()
                                        .color(style::TEXT_PRIMARY),
                                );
                                widgets::hint(
                                    ui,
                                    &format!(
                                        "“{}” — {} for {} ({} per card), booked as today's revenue.",
                                        done.name,
                                        widgets::cards(done.copies),
                                        widgets::euro(done.price),
                                        widgets::share(per_card_share(done.price, done.copies))
                                    ),
                                );
                            });
                        });
                        ui.add_space(14.0);
                        ui.label(egui::RichText::new("TCG Powertools CSV").strong());
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(done.csv.display().to_string())
                                        .monospace()
                                        .color(style::TEXT_MUTED),
                                )
                                .truncate(),
                            );
                            if ui.small_button("Copy path").clicked() {
                                ui.ctx().copy_text(done.csv.display().to_string());
                            }
                        });

                        ui.add_space(14.0);
                        ui.label(egui::RichText::new("Next steps").strong());
                        ui.add_space(2.0);
                        for (n, text) in [
                            "Import the CSV in TCG Powertools — it removes the cards from your \
                             Cardmarket stock.",
                            "Put the cards in the lot and list it on Cardmarket.",
                            "Load your next inventory export as usual. The removal is matched \
                             automatically and never counts as a sale; the lot then shows \
                             “Removed” in the history.",
                        ]
                        .iter()
                        .enumerate()
                        {
                            ui.horizontal_top(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("{}.", n + 1)).color(style::ACCENT),
                                );
                                ui.add(
                                    egui::Label::new(egui::RichText::new(*text).color(style::TEXT_MUTED))
                                        .wrap(),
                                );
                            });
                        }

                        ui.add_space(18.0);
                        ui.horizontal(|ui| {
                            another = style::primary_button(ui, "Build another lot").clicked();
                            history = style::secondary_button(ui, "View history").clicked();
                        });
                    });
            });
        });
        if another {
            state.build_another();
        }
        if history {
            state.build_another();
            state.tab = Tab::History;
        }
    });
}
