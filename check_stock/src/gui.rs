use eframe::egui;
use egui::ViewportBuilder;

pub struct StockCheckerApp {
    inventory_path: String,
    wantslist_path: String,
    output: String,
    save_to_file: bool,
}

impl Default for StockCheckerApp {
    fn default() -> Self {
        Self {
            inventory_path: String::new(),
            wantslist_path: String::new(),
            output: String::new(),
            save_to_file: false,
        }
    }
}

impl eframe::App for StockCheckerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("MTG Stock Checker");
            
            ui.horizontal(|ui| {
                ui.label("Inventory CSV:");
                if ui.button("Browse").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("CSV", &["csv"])
                        .pick_file() {
                            self.inventory_path = path.display().to_string();
                        }
                }
                ui.text_edit_singleline(&mut self.inventory_path);
            });

            ui.horizontal(|ui| {
                ui.label("Wantslist:");
                if ui.button("Browse").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .pick_file() {
                            self.wantslist_path = path.display().to_string();
                        }
                }
                ui.text_edit_singleline(&mut self.wantslist_path);
            });

            ui.checkbox(&mut self.save_to_file, "Save output to file");

            if ui.button("Check Stock").clicked() {
                match self.check_stock() {
                    Ok(output) => self.output = output,
                    Err(e) => self.output = format!("Error: {}", e),
                }
            }

            ui.separator();

            // Display the output in a scrollable area
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.output)
                    .desired_width(f32::INFINITY)
                    .desired_rows(20)
                    .font(egui::TextStyle::Monospace));
            });
        });
    }
}

impl StockCheckerApp {
    fn check_stock(&self) -> Result<String, Box<dyn std::error::Error>> {
        use crate::{Args, run_with_args};

        if self.inventory_path.is_empty() || self.wantslist_path.is_empty() {
            return Err("Please select both inventory and wantslist files".into());
        }

        let args = Args {
            inventory_csv: Some(self.inventory_path.clone()),
            wantslist: Some(self.wantslist_path.clone()),
            write_output: self.save_to_file,
        };

        run_with_args(&args)
    }
}

pub fn launch_gui() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "MTG Stock Checker",
        options,
        Box::new(|_cc| Box::new(StockCheckerApp::default())),
    )
}