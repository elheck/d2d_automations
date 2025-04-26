use eframe::egui;
use egui::ViewportBuilder;

#[derive(PartialEq)]
enum Language {
    English,
    German,
    Spanish,
    French,
    Italian,
}

impl Language {
    fn as_str(&self) -> &'static str {
        match self {
            Language::English => "English",
            Language::German => "German",
            Language::Spanish => "Spanish",
            Language::French => "French",
            Language::Italian => "Italian",
        }
    }
}

pub struct StockCheckerApp {
    inventory_path: String,
    wantslist_path: String,
    output: String,
    save_to_file: bool,
    preferred_language: Language,
    show_picking_list: bool,
}

impl Default for StockCheckerApp {
    fn default() -> Self {
        Self {
            inventory_path: String::new(),
            wantslist_path: String::new(),
            output: String::new(),
            save_to_file: false,
            preferred_language: Language::English,
            show_picking_list: false,
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

            ui.horizontal(|ui| {
                ui.label("Preferred Language:");
                egui::ComboBox::new("language_selector", "")
                    .selected_text(self.preferred_language.as_str())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.preferred_language, Language::English, "English");
                        ui.selectable_value(&mut self.preferred_language, Language::German, "German");
                        ui.selectable_value(&mut self.preferred_language, Language::Spanish, "Spanish");
                        ui.selectable_value(&mut self.preferred_language, Language::French, "French");
                        ui.selectable_value(&mut self.preferred_language, Language::Italian, "Italian");
                    });
            });

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.save_to_file, "Save output to file");
                ui.separator();
                ui.checkbox(&mut self.show_picking_list, "Show as picking list");
            });

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
        if self.inventory_path.is_empty() || self.wantslist_path.is_empty() {
            return Err("Please select both inventory and wantslist files".into());
        }

        let lang_code = match self.preferred_language {
            Language::English => "en",
            Language::German => "de",
            Language::Spanish => "es",
            Language::French => "fr",
            Language::Italian => "it",
        };

        let args = crate::Args {
            inventory_csv: Some(self.inventory_path.clone()),
            wantslist: Some(self.wantslist_path.clone()),
            write_output: self.save_to_file,
            language: Some(lang_code.to_string()),
        };

        // Run stock check and get normal output
        let normal_output = crate::run_with_args(&args)?;

        if self.show_picking_list {
            // For picking list, we need to process the inventory again
            let inventory = d2d_automations::read_csv(&self.inventory_path)?;
            let wantslist = crate::read_wantslist(&self.wantslist_path)?;
            
            // Find all matching cards
            let mut all_matching_cards = Vec::new();
            for (_, card_name) in wantslist {
                let matching_cards: Vec<_> = inventory.iter()
                    .filter(|card| {
                        let name = crate::get_card_name(card, Some(lang_code));
                        name.eq_ignore_ascii_case(&card_name)
                    })
                    .collect();
                all_matching_cards.extend(matching_cards);
            }

            Ok(crate::generate_picking_list(&all_matching_cards))
        } else {
            Ok(normal_output)
        }
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
        Box::new(|_cc| Ok(Box::new(StockCheckerApp::default()))),
    )
}