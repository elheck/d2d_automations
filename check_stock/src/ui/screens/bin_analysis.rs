use crate::{
    io::read_csv,
    stock_analysis::{format_stock_analysis_with_sort, SortOrder, StockAnalysis},
    ui::{
        components::FilePicker,
        state::{BinAnalysisState, Screen},
        style,
    },
};
use eframe::egui;
use log::info;

pub struct BinAnalysisScreen;

impl BinAnalysisScreen {
    pub fn show(ctx: &egui::Context, current_screen: &mut Screen, state: &mut BinAnalysisState) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("bin_analysis_scroll")
                .show(ui, |ui| {
                    if style::back_button(ui, "Back") {
                        *current_screen = Screen::Welcome;
                    }
                    ui.add_space(8.0);

                    style::screen_heading(ui, "Bin Capacity Analysis");

                    // ── File picker ─────────────────────────────────────────
                    style::section_frame().show(ui, |ui| {
                        FilePicker::new("Inventory CSV:", &mut state.inventory_path)
                            .with_filter("CSV", &["csv"])
                            .show(ui);
                    });

                    ui.add_space(10.0);

                    // ── Controls ────────────────────────────────────────────
                    style::section_frame().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Minimum Free Slots:");
                            ui.add(egui::Slider::new(&mut state.free_slots, 1..=30).text("slots"));
                        });

                        ui.add_space(5.0);

                        ui.horizontal(|ui| {
                            ui.label("Sort by:");
                            egui::ComboBox::from_label("")
                                .selected_text(match state.sort_order {
                                    SortOrder::ByFreeSlots => "Free Slots (Descending)",
                                    SortOrder::ByLocation => "Location (Ascending)",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut state.sort_order,
                                        SortOrder::ByFreeSlots,
                                        "Free Slots (Descending)",
                                    );
                                    ui.selectable_value(
                                        &mut state.sort_order,
                                        SortOrder::ByLocation,
                                        "Location (Ascending)",
                                    );
                                });
                        });

                        ui.add_space(10.0);

                        if style::primary_button(ui, "Analyze Stock").clicked() {
                            if let Err(e) = Self::analyze_stock(state) {
                                state.output = format!("Error: {e}");
                            }
                        }
                    });

                    ui.add_space(8.0);
                    ui.separator();

                    if !state.output.is_empty() {
                        ui.add_space(6.0);
                        if style::secondary_button(ui, "Save Analysis to File").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name("bin_analysis.txt")
                                .add_filter("Text Files", &["txt"])
                                .save_file()
                            {
                                if let Err(e) = std::fs::write(&path, &state.output) {
                                    state.output = format!("Error saving file: {e}");
                                }
                            }
                        }
                        ui.add_space(4.0);
                        ui.add(
                            egui::TextEdit::multiline(&mut state.output)
                                .desired_width(f32::INFINITY)
                                .desired_rows(20)
                                .font(egui::TextStyle::Monospace),
                        );
                    }
                });
        });
    }

    fn analyze_stock(state: &mut BinAnalysisState) -> Result<(), Box<dyn std::error::Error>> {
        if state.inventory_path.is_empty() {
            return Err("Please select an inventory file".into());
        }

        info!(
            "Starting bin analysis with {} free slots threshold",
            state.free_slots
        );

        let inventory = read_csv(&state.inventory_path)?;
        let analyzer = StockAnalysis::new(inventory);
        let stats = analyzer.analyze_with_free_slots(state.free_slots);

        info!(
            "Found {} bins with {} or more free slots",
            stats.available_bins.len(),
            state.free_slots
        );

        state.output = format_stock_analysis_with_sort(&stats, state.sort_order);
        Ok(())
    }
}
