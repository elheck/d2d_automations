use crate::{
    io::read_csv,
    ui::{
        components::FilePicker,
        state::{AppState, PricingState, Screen},
        style,
    },
};
use eframe::egui;
use log::info;

pub struct PricingScreen;

impl PricingScreen {
    pub fn show(ctx: &egui::Context, app_state: &mut AppState, state: &mut PricingState) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if style::back_button(ui, "Back") {
                app_state.current_screen = Screen::Welcome;
            }
            ui.add_space(8.0);

            style::screen_heading(ui, "Stock Pricing");

            // ── File picker ─────────────────────────────────────────────────
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
            });

            ui.add_space(10.0);

            if !state.cards.is_empty() {
                ui.label(format!("Loaded {} cards", state.cards.len()));
                ui.add_space(10.0);

                // Placeholder: pricing functions will go here
                ui.add_enabled_ui(true, |ui| {
                    style::section_frame().show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Pricing Tools")
                                .strong()
                                .color(style::TEXT_PRIMARY),
                        );
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("Coming soon…").color(style::TEXT_MUTED));
                    });
                });
            } else {
                style::section_frame().show(ui, |ui| {
                    ui.add_enabled_ui(false, |ui| {
                        ui.label(
                            egui::RichText::new("Pricing Tools")
                                .strong()
                                .color(style::TEXT_PRIMARY),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("Load a CSV file to enable pricing tools.")
                                .color(style::TEXT_MUTED),
                        );
                    });
                });
            }
        });
    }

    fn load_csv(state: &mut PricingState) {
        info!("Loading CSV for pricing: {}", state.csv_path);
        match read_csv(&state.csv_path) {
            Ok(cards) => {
                info!("Loaded {} cards for pricing", cards.len());
                state.cards = cards;
            }
            Err(e) => {
                log::error!("Error loading CSV: {}", e);
                state.load_error = Some(e.to_string());
            }
        }
    }
}
