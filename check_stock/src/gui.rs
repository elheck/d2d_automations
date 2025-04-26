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
    preferred_language: Language,
    show_picking_list: bool,
}

impl Default for StockCheckerApp {
    fn default() -> Self {
        Self {
            inventory_path: String::new(),
            wantslist_path: String::new(),
            output: String::new(),
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
                ui.checkbox(&mut self.show_picking_list, "Show as picking list");
            });

            ui.horizontal(|ui| {
                if ui.button("Check Stock").clicked() {
                    match self.check_stock() {
                        Ok(output) => self.output = output,
                        Err(e) => self.output = format!("Error: {}", e),
                    }
                }

                if !self.output.is_empty() {
                    if ui.button("Save Output").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name("stock_check_output.txt")
                            .add_filter("Text Files", &["txt"])
                            .save_file() {
                                if let Err(e) = std::fs::write(&path, &self.output) {
                                    self.output = format!("Error saving file: {}\n\n{}", e, self.output);
                                }
                        }
                    }
                }
            });

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
            write_output: false, // Always false since we handle saving separately
            language: Some(lang_code.to_string()),
        };

        if self.show_picking_list {
            // Process inventory and wantslist just like in run_with_args
            let inventory = d2d_automations::read_csv(&self.inventory_path)?;
            let wantslist = crate::read_wantslist(&self.wantslist_path)?;
            
            let mut all_matching_cards = Vec::new();
            
            // Process each card in the wantslist (same as run_with_args)
            for (needed_quantity, card_name) in wantslist {
                // Find matching cards in preferred language
                let matching_cards: Vec<_> = inventory.iter()
                    .filter(|card| {
                        let name = crate::get_card_name(card, Some(lang_code));
                        name.eq_ignore_ascii_case(&card_name)
                    })
                    .collect();

                if !matching_cards.is_empty() {
                    // Group cards by set and price, just like run_with_args
                    let mut cards_by_set: std::collections::HashMap<String, Vec<&d2d_automations::Card>> = std::collections::HashMap::new();
                    for card in &matching_cards {
                        let set_key = format!("{} ({})", &card.set, &card.set_code);
                        cards_by_set.entry(set_key).or_default().push(card);
                    }

                    let mut remaining_needed = needed_quantity;

                    // Sort sets by price to prioritize cheaper versions
                    let mut sets: Vec<_> = cards_by_set.iter().collect();
                    sets.sort_by(|a, b| {
                        let price_a = a.1[0].price.parse::<f64>().unwrap_or(f64::MAX);
                        let price_b = b.1[0].price.parse::<f64>().unwrap_or(f64::MAX);
                        price_a.partial_cmp(&price_b).unwrap()
                    });

                    // Add cards from each set until we have enough
                    for (_, cards) in sets {
                        if remaining_needed <= 0 {
                            break;
                        }

                        let mut cards_vec = cards.clone();
                        cards_vec.sort_by(|a, b| {
                            let price_a = a.price.parse::<f64>().unwrap_or(f64::MAX);
                            let price_b = b.price.parse::<f64>().unwrap_or(f64::MAX);
                            price_a.partial_cmp(&price_b).unwrap()
                        });

                        for card in cards_vec {
                            if remaining_needed <= 0 {
                                break;
                            }
                            if let Ok(quantity) = card.quantity.parse::<i32>() {
                                if quantity > 0 {
                                    let copies = remaining_needed.min(quantity);
                                    all_matching_cards.push(card);
                                    remaining_needed -= copies;
                                }
                            }
                        }
                    }
                }
            }

            Ok(crate::generate_picking_list(&all_matching_cards))
        } else {
            // Regular output mode
            crate::run_with_args(&args)
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