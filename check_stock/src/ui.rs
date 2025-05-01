use eframe::egui;
use egui::ViewportBuilder;
use crate::card_matching::find_matching_cards;
use crate::formatters::{format_regular_output, format_picking_list, format_invoice_list, format_update_stock_csv};
use crate::io::{read_csv, read_wantslist};

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

    fn code(&self) -> &'static str {
        match self {
            Language::English => "en",
            Language::German => "de",
            Language::Spanish => "es",
            Language::French => "fr",
            Language::Italian => "it",
        }
    }
}

#[derive(PartialEq)]
enum OutputFormat {
    Regular,
    PickingList,
    InvoiceList,
    UpdateStock,
}

impl OutputFormat {
    fn as_str(&self) -> &'static str {
        match self {
            OutputFormat::Regular => "Regular",
            OutputFormat::PickingList => "Picking List",
            OutputFormat::InvoiceList => "Invoice List",
            OutputFormat::UpdateStock => "Update Stock CSV",
        }
    }
}

pub struct StockCheckerApp {
    inventory_path: String,
    wantslist_path: String,
    output: String,
    preferred_language: Language,
    all_matches: Vec<(String, Vec<(crate::models::Card, i32, String)>)>,
    selected: Vec<bool>,
    show_selection: bool,
    selection_format: Option<OutputFormat>,
    selection_mode: bool,  // New field to track if we're in selection mode
    show_output_window: bool,
    output_window_content: String,
    output_window_title: String,
}

impl Default for StockCheckerApp {
    fn default() -> Self {
        Self {
            inventory_path: String::new(),
            wantslist_path: String::new(),
            output: String::new(),
            preferred_language: Language::English,
            all_matches: Vec::new(),
            selected: Vec::new(),
            show_selection: false,
            selection_format: None,
            selection_mode: false,
            show_output_window: false,
            output_window_content: String::new(),
            output_window_title: String::new(),
        }
    }
}

impl eframe::App for StockCheckerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Show output window if enabled
        if self.show_output_window {
            egui::Window::new(&self.output_window_title)
                .default_size([800.0, 600.0])
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(ui.available_height() - 40.0)
                        .show(ui, |ui| {
                            ui.add(egui::TextEdit::multiline(&mut self.output_window_content)
                                .desired_width(f32::INFINITY)
                                .desired_rows(20)
                                .font(egui::TextStyle::Monospace));
                        });
                    
                    ui.horizontal(|ui| {
                        if ui.button("Close").clicked() {
                            self.show_output_window = false;
                        }
                        if ui.button("Save to File").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name(match self.output_window_title.as_str() {
                                    "Picking List" => "picking_list.txt",
                                    "Invoice List" => "invoice_list.txt",
                                    "Stock Update" => "stock_update.csv",
                                    _ => "output.txt",
                                })
                                .add_filter(
                                    if self.output_window_title == "Stock Update" { 
                                        "CSV Files"
                                    } else {
                                        "Text Files"
                                    },
                                    &[if self.output_window_title == "Stock Update" { "csv" } else { "txt" }]
                                )
                                .save_file() {
                                    if let Err(e) = std::fs::write(&path, &self.output_window_content) {
                                        self.output = format!("Error saving file: {}", e);
                                    }
                            }
                        }
                    });
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("MTG Stock Checker");
            
            // File pickers
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
                if ui.button("Check Stock").clicked() {
                    match self.check_stock() {
                        Ok(_) => {},
                        Err(e) => {
                            self.output = format!("Error: {}", e);
                        }
                    }
                }
            });

            ui.separator();

            if self.show_selection || self.selection_mode {
                ui.label("Select the cards you want to include:");
                egui::ScrollArea::vertical()
                    .max_height(ui.available_height() - 50.0)
                    .show(ui, |ui| {
                        let mut idx = 0;
                        for (card_name, cards) in &self.all_matches {
                            if !cards.is_empty() {
                                ui.label(format!("{}:", card_name));
                                for (card, quantity, set_name) in cards {
                                    let mut checked = self.selected[idx];
                                    let label = format!(
                                        "{} {} [{}] from {} - {} condition - {:.2} €{}",
                                        quantity,
                                        card.name,
                                        card.language,
                                        set_name,
                                        card.condition,
                                        card.price.parse::<f64>().unwrap_or(0.0),
                                        card.location.as_ref()
                                            .filter(|loc| !loc.trim().is_empty())
                                            .map(|loc| format!(" [Location: {}]", loc))
                                            .unwrap_or_default()
                                    );
                                    if ui.checkbox(&mut checked, label).changed() {
                                        self.selected[idx] = checked;
                                    }
                                    idx += 1;
                                }
                                ui.add_space(4.0);
                            }
                        }
                    });

                ui.separator();
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Generate Picking List").clicked() {
                            self.generate_selected_output_in_window(OutputFormat::PickingList);
                        }
                        if ui.button("Generate Invoice List").clicked() {
                            self.generate_selected_output_in_window(OutputFormat::InvoiceList);
                        }
                        if ui.button("Generate Stock Update CSV").clicked() {
                            self.generate_selected_output_in_window(OutputFormat::UpdateStock);
                        }
                        if ui.button("Return to Regular List").clicked() {
                            self.show_selection = false;
                            self.selection_mode = false;
                            self.generate_regular_output();
                        }
                    });
                });
            } else if !self.output.is_empty() {
                egui::ScrollArea::vertical()
                    .max_height(ui.available_height() - 50.0)
                    .show(ui, |ui| {
                        ui.add(egui::TextEdit::multiline(&mut self.output)
                            .desired_width(f32::INFINITY)
                            .desired_rows(20)
                            .font(egui::TextStyle::Monospace));
                    });

                if !self.all_matches.is_empty() {
                    ui.separator();
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Select Cards for Lists").clicked() {
                                self.start_selection();
                            }
                        });
                    });
                }
            }
        });
    }
}

impl StockCheckerApp {
    fn check_stock(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.inventory_path.is_empty() || self.wantslist_path.is_empty() {
            return Err("Please select both inventory and wantslist files".into());
        }
        let inventory = read_csv(&self.inventory_path)?;
        let wantslist = read_wantslist(&self.wantslist_path)?;
        self.all_matches.clear();
        
        for wants_entry in wantslist {
            let matched_cards = find_matching_cards(
                &wants_entry.name,
                wants_entry.quantity,
                &inventory,
                Some(self.preferred_language.code())
            );
            
            let owned_cards = matched_cards
                .into_iter()
                .map(|mc| {
                    let card = (*mc.card).clone();
                    (card, mc.quantity, mc.set_name)
                })
                .collect();
                
            self.all_matches.push((wants_entry.name, owned_cards));
        }

        // Generate regular output immediately
        let selected_matches: Vec<_> = self.all_matches.iter()
            .map(|(name, cards)| {
                let group_cards: Vec<_> = cards.iter()
                    .map(|(card, quantity, set_name)| crate::card_matching::MatchedCard {
                        card,
                        quantity: *quantity,
                        set_name: set_name.clone(),
                    })
                    .collect();
                (name.clone(), group_cards)
            })
            .collect();
        
        self.output = format_regular_output(&selected_matches);
        Ok(())
    }

    fn start_selection(&mut self) {
        if self.selected.is_empty() {
            // Only initialize selection if not already set
            self.selected = std::iter::repeat(true)
                .take(self.all_matches.iter().map(|(_, cards)| cards.len()).sum())
                .collect();
        }
        self.show_selection = true;
        self.selection_mode = true;
    }

    fn generate_selected_output(&mut self, format: OutputFormat) {
        let mut selected_matches = Vec::new();
        let mut idx = 0;
        
        for (name, cards) in &self.all_matches {
            let mut group_cards = Vec::new();
            for (card, quantity, set_name) in cards {
                if self.selected[idx] {
                    group_cards.push(crate::card_matching::MatchedCard {
                        card,
                        quantity: *quantity,
                        set_name: set_name.clone(),
                    });
                }
                idx += 1;
            }
            if !group_cards.is_empty() {
                selected_matches.push((name.clone(), group_cards));
            }
        }

        self.output = match format {
            OutputFormat::Regular => format_regular_output(&selected_matches),
            OutputFormat::PickingList => {
                let all_cards: Vec<_> = selected_matches.iter()
                    .flat_map(|(_, cards)| cards.iter().cloned())
                    .collect();
                format_picking_list(&all_cards)
            },
            OutputFormat::InvoiceList => {
                let all_cards: Vec<_> = selected_matches.iter()
                    .flat_map(|(_, cards)| cards.iter().cloned())
                    .collect();
                format_invoice_list(&all_cards)
            },
            OutputFormat::UpdateStock => {
                let all_cards: Vec<_> = selected_matches.iter()
                    .flat_map(|(_, cards)| cards.iter().cloned())
                    .collect();
                format_update_stock_csv(&all_cards)
            }
        };
    }

    fn generate_regular_output(&mut self) {
        let selected_matches: Vec<_> = self.all_matches.iter()
            .map(|(name, cards)| {
                let group_cards: Vec<_> = cards.iter()
                    .map(|(card, quantity, set_name)| crate::card_matching::MatchedCard {
                        card,
                        quantity: *quantity,
                        set_name: set_name.clone(),
                    })
                    .collect();
                (name.clone(), group_cards)
            })
            .collect();
        
        self.output = format_regular_output(&selected_matches);
    }

    fn generate_selected_output_in_window(&mut self, format: OutputFormat) {
        let mut selected_matches = Vec::new();
        let mut idx = 0;
        
        for (name, cards) in &self.all_matches {
            let mut group_cards = Vec::new();
            for (card, quantity, set_name) in cards {
                if self.selected[idx] {
                    group_cards.push(crate::card_matching::MatchedCard {
                        card,
                        quantity: *quantity,
                        set_name: set_name.clone(),
                    });
                }
                idx += 1;
            }
            if !group_cards.is_empty() {
                selected_matches.push((name.clone(), group_cards));
            }
        }

        self.output_window_content = match format {
            OutputFormat::Regular => format_regular_output(&selected_matches),
            OutputFormat::PickingList => {
                let all_cards: Vec<_> = selected_matches.iter()
                    .flat_map(|(_, cards)| cards.iter().cloned())
                    .collect();
                format_picking_list(&all_cards)
            },
            OutputFormat::InvoiceList => {
                let all_cards: Vec<_> = selected_matches.iter()
                    .flat_map(|(_, cards)| cards.iter().cloned())
                    .collect();
                format_invoice_list(&all_cards)
            },
            OutputFormat::UpdateStock => {
                let all_cards: Vec<_> = selected_matches.iter()
                    .flat_map(|(_, cards)| cards.iter().cloned())
                    .collect();
                format_update_stock_csv(&all_cards)
            }
        };

        self.output_window_title = match format {
            OutputFormat::Regular => "Regular List",
            OutputFormat::PickingList => "Picking List",
            OutputFormat::InvoiceList => "Invoice List",
            OutputFormat::UpdateStock => "Stock Update",
        }.to_string();

        self.show_output_window = true;
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