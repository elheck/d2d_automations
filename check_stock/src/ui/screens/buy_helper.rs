use crate::{
    buy_helper::{classify, compute_summary, export_csv, CardClass},
    io::read_csv,
    ui::{
        components::{FilePicker, OutputWindow},
        state::{BuyHelperState, Screen},
        style,
    },
};
use eframe::egui;

/// Maximum number of single cards shown in the on-screen preview table. The
/// full breakdown is always available via the exported CSV.
const PREVIEW_LIMIT: usize = 200;

pub struct BuyHelperScreen;

impl BuyHelperScreen {
    pub fn show(ctx: &egui::Context, current_screen: &mut Screen, state: &mut BuyHelperState) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("buy_helper_scroll")
                .show(ui, |ui| {
                    if style::back_button(ui, "Back") {
                        *current_screen = Screen::Welcome;
                    }
                    ui.add_space(8.0);

                    style::screen_heading(ui, "Card Buy Helper");

                    // Read-only reassurance — this tool never touches the DB.
                    ui.label(
                        egui::RichText::new(
                            "Read-only: cards are only used to calculate an offer and are never \
                             written to the inventory database.",
                        )
                        .color(style::TEXT_MUTED)
                        .size(12.0),
                    );
                    ui.add_space(10.0);

                    // ── File input ──────────────────────────────────────────
                    style::section_frame().show(ui, |ui| {
                        let picked = FilePicker::new("Card export CSV:", &mut state.csv_path)
                            .with_filter("CSV", &["csv"])
                            .show(ui);
                        ui.add_space(6.0);
                        let reload = style::secondary_button(ui, "Load CSV").clicked();
                        if picked || reload {
                            Self::load(state);
                        }
                        if !state.cards.is_empty() {
                            ui.add_space(4.0);
                            style::status_ok(
                                ui,
                                &format!("Loaded {} card rows", state.cards.len()),
                            );
                        }
                    });

                    if let Some(err) = &state.load_error {
                        ui.add_space(6.0);
                        style::status_error(ui, &format!("Error: {err}"));
                    }

                    if state.cards.is_empty() {
                        return;
                    }

                    let params = state.params();
                    let summary = compute_summary(&state.cards, &params);

                    ui.add_space(10.0);
                    Self::show_controls(ui, state);
                    ui.add_space(10.0);
                    Self::show_summary(ui, &summary);
                    ui.add_space(10.0);
                    Self::show_actions(ui, state);
                    ui.add_space(10.0);
                    Self::show_preview(ui, state);
                });
        });

        if state.show_output_window {
            OutputWindow::new(
                "Buy Offer — Detailed CSV",
                &mut state.output_content,
                &mut state.show_output_window,
                "csv",
            )
            .show(ctx);
        }
    }

    fn load(state: &mut BuyHelperState) {
        if state.csv_path.trim().is_empty() {
            state.load_error = Some("Please select a CSV file".to_string());
            return;
        }
        match read_csv(&state.csv_path) {
            Ok(cards) => {
                state.cards = cards;
                state.load_error = None;
            }
            Err(e) => {
                state.cards.clear();
                state.load_error = Some(e.to_string());
            }
        }
    }

    fn show_controls(ui: &mut egui::Ui, state: &mut BuyHelperState) {
        style::section_frame().show(ui, |ui| {
            ui.label(
                egui::RichText::new("Singles selection")
                    .color(style::TEXT_PRIMARY)
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "Cards matching any rule below are valued individually; the rest are bulk.",
                )
                .color(style::TEXT_MUTED)
                .size(12.0),
            );
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label("Rarities:");
                ui.checkbox(&mut state.single_common, "Common");
                ui.checkbox(&mut state.single_uncommon, "Uncommon");
                ui.checkbox(&mut state.single_rare, "Rare");
                ui.checkbox(&mut state.single_mythic, "Mythic");
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.use_min_price, "Value at or above");
                ui.add_enabled(
                    state.use_min_price,
                    egui::DragValue::new(&mut state.min_price)
                        .speed(0.05)
                        .range(0.0..=f64::MAX)
                        .prefix("€ ")
                        .max_decimals(2),
                );
                ui.label("counts as a single");
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Offer rates")
                    .color(style::TEXT_PRIMARY)
                    .strong(),
            );
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label("Buy singles at");
                ui.add(
                    egui::DragValue::new(&mut state.single_buy_percent)
                        .speed(1.0)
                        .range(0.0..=100.0)
                        .suffix(" %"),
                );
                ui.label("of market value");
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Bulk rate: €");
                ui.add(
                    egui::DragValue::new(&mut state.bulk_rate)
                        .speed(0.5)
                        .range(0.0..=f64::MAX)
                        .max_decimals(2),
                );
                ui.label("per");
                ui.add(
                    egui::DragValue::new(&mut state.bulk_batch)
                        .speed(10.0)
                        .range(1..=u32::MAX),
                );
                ui.label("cards");
            });
        });
    }

    fn show_summary(ui: &mut egui::Ui, summary: &crate::buy_helper::BuySummary) {
        style::section_frame().show(ui, |ui| {
            ui.label(
                egui::RichText::new("Offer summary")
                    .color(style::TEXT_PRIMARY)
                    .strong(),
            );
            ui.add_space(8.0);

            egui::Grid::new("buy_helper_summary_grid")
                .num_columns(4)
                .spacing([18.0, 6.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("").color(style::TEXT_MUTED));
                    ui.label(egui::RichText::new("Cards").color(style::TEXT_MUTED));
                    ui.label(egui::RichText::new("Market value").color(style::TEXT_MUTED));
                    ui.label(egui::RichText::new("Offer").color(style::TEXT_MUTED));
                    ui.end_row();

                    ui.label("Singles");
                    ui.label(format!(
                        "{} ({} rows)",
                        summary.single_cards, summary.single_rows
                    ));
                    ui.label(format!("€ {:.2}", summary.single_market_value));
                    ui.label(format!("€ {:.2}", summary.single_offer));
                    ui.end_row();

                    ui.label("Bulk");
                    ui.label(format!(
                        "{} ({} rows)",
                        summary.bulk_cards, summary.bulk_rows
                    ));
                    ui.label(format!("€ {:.2}", summary.bulk_market_value));
                    ui.label(format!("€ {:.2}", summary.bulk_offer));
                    ui.end_row();
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!("Total offer: € {:.2}", summary.total_offer))
                    .color(style::COLOR_SUCCESS)
                    .size(18.0)
                    .strong(),
            );
        });
    }

    fn show_actions(ui: &mut egui::Ui, state: &mut BuyHelperState) {
        ui.horizontal(|ui| {
            if style::primary_button(ui, "Save Offer CSV").clicked() {
                let params = state.params();
                match export_csv(&state.cards, &params) {
                    Ok(content) => {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name("buy_offer.csv")
                            .add_filter("CSV Files", &["csv"])
                            .save_file()
                        {
                            if let Err(e) = std::fs::write(&path, &content) {
                                state.load_error = Some(format!("Error saving file: {e}"));
                            }
                        }
                    }
                    Err(e) => state.load_error = Some(format!("Export failed: {e}")),
                }
            }

            if style::secondary_button(ui, "View Detailed CSV").clicked() {
                let params = state.params();
                match export_csv(&state.cards, &params) {
                    Ok(content) => {
                        state.output_content = content;
                        state.show_output_window = true;
                    }
                    Err(e) => state.load_error = Some(format!("Export failed: {e}")),
                }
            }
        });
    }

    fn show_preview(ui: &mut egui::Ui, state: &BuyHelperState) {
        let params = state.params();

        // Collect single cards, most valuable first, for a quick sanity check.
        let mut singles: Vec<&crate::models::Card> = state
            .cards
            .iter()
            .filter(|c| classify(c, &params) == CardClass::Single)
            .collect();
        singles.sort_by(|a, b| {
            b.price_f64()
                .partial_cmp(&a.price_f64())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        style::section_frame().show(ui, |ui| {
            ui.label(
                egui::RichText::new("Singles preview (highest value first)")
                    .color(style::TEXT_PRIMARY)
                    .strong(),
            );
            if singles.len() > PREVIEW_LIMIT {
                ui.label(
                    egui::RichText::new(format!(
                        "Showing top {PREVIEW_LIMIT} of {} — full list is in the exported CSV.",
                        singles.len()
                    ))
                    .color(style::TEXT_MUTED)
                    .size(12.0),
                );
            }
            ui.add_space(6.0);

            egui::ScrollArea::vertical()
                .id_salt("buy_helper_preview_scroll")
                .max_height(280.0)
                .show(ui, |ui| {
                    egui::Grid::new("buy_helper_preview_grid")
                        .num_columns(5)
                        .striped(true)
                        .spacing([14.0, 4.0])
                        .show(ui, |ui| {
                            for header in ["Name", "Set", "Rarity", "Qty", "Unit €"] {
                                ui.label(egui::RichText::new(header).color(style::TEXT_MUTED));
                            }
                            ui.end_row();

                            for card in singles.iter().take(PREVIEW_LIMIT) {
                                ui.label(&card.name);
                                ui.label(&card.set_code);
                                ui.label(&card.rarity);
                                ui.label(card.quantity.trim());
                                ui.label(format!("{:.2}", card.price_f64()));
                                ui.end_row();
                            }
                        });
                });
        });
    }
}
