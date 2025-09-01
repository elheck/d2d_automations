use crate::{
    io::read_csv,
    stock_analysis::{format_stock_analysis_with_sort, SortOrder, StockAnalysis},
    ui::{
        components::FilePicker,
        state::{Screen, StockAnalysisState},
    },
};
use eframe::egui;

pub struct StockAnalysisScreen;

impl StockAnalysisScreen {
    pub fn show(ctx: &egui::Context, current_screen: &mut Screen, state: &mut StockAnalysisState) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("← Back to Welcome Screen").clicked() {
                    *current_screen = Screen::Welcome;
                }
            });
            ui.add_space(10.0);

            ui.heading("Stock Analysis");
            ui.add_space(10.0);

            FilePicker::new("Inventory CSV:", &mut state.inventory_path)
                .with_filter("CSV", &["csv"])
                .show(ui);

            ui.add_space(10.0);

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

            if ui.button("Analyze Stock").clicked() {
                if let Err(e) = Self::analyze_stock(state) {
                    state.output = format!("Error: {e}");
                }
            }

            ui.separator();

            if !state.output.is_empty() {
                egui::ScrollArea::vertical()
                    .max_height(ui.available_height())
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut state.output)
                                .desired_width(f32::INFINITY)
                                .desired_rows(20)
                                .font(egui::TextStyle::Monospace),
                        );
                    });

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    if ui.button("Save Analysis to File").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name("stock_analysis.txt")
                            .add_filter("Text Files", &["txt"])
                            .save_file()
                        {
                            if let Err(e) = std::fs::write(&path, &state.output) {
                                state.output = format!("Error saving file: {e}");
                            }
                        }
                    }
                });
            }
        });
    }

    fn analyze_stock(state: &mut StockAnalysisState) -> Result<(), Box<dyn std::error::Error>> {
        if state.inventory_path.is_empty() {
            return Err("Please select an inventory file".into());
        }

        let inventory = read_csv(&state.inventory_path)?;
        let analyzer = StockAnalysis::new(inventory);
        let stats = analyzer.analyze_with_free_slots(state.free_slots);
        state.output = format_stock_analysis_with_sort(&stats, state.sort_order);
        Ok(())
    }
}
