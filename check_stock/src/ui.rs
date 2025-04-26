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
    output_format: OutputFormat,
}

impl Default for StockCheckerApp {
    fn default() -> Self {
        Self {
            inventory_path: String::new(),
            wantslist_path: String::new(),
            output: String::new(),
            preferred_language: Language::English,
            output_format: OutputFormat::Regular,
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
                ui.label("Output Format:");
                egui::ComboBox::new("output_format", "")
                    .selected_text(self.output_format.as_str())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.output_format, OutputFormat::Regular, "Regular");
                        ui.selectable_value(&mut self.output_format, OutputFormat::PickingList, "Picking List");
                        ui.selectable_value(&mut self.output_format, OutputFormat::InvoiceList, "Invoice List");
                        ui.selectable_value(&mut self.output_format, OutputFormat::UpdateStock, "Update Stock CSV");
                    });
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
                        let default_name = match self.output_format {
                            OutputFormat::UpdateStock => "stock_update.csv",
                            _ => "stock_check_output.txt"
                        };
                        
                        let file_dialog = rfd::FileDialog::new()
                            .set_file_name(default_name);
                            
                        let file_dialog = match self.output_format {
                            OutputFormat::UpdateStock => file_dialog.add_filter("CSV Files", &["csv"]),
                            _ => file_dialog.add_filter("Text Files", &["txt"])
                        };

                        if let Some(path) = file_dialog.save_file() {
                            if let Err(e) = std::fs::write(path, &self.output) {
                                self.output = format!("Error saving file: {}", e);
                            }
                        }
                    }
                }
            });

            ui.separator();

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

        let inventory = read_csv(&self.inventory_path)?;
        let wantslist = read_wantslist(&self.wantslist_path)?;
        
        let mut all_matches = Vec::new();

        // Process each card in the wantslist
        for wants_entry in wantslist {
            let matched_cards = find_matching_cards(
                &wants_entry.name,
                wants_entry.quantity,
                &inventory,
                Some(self.preferred_language.code())
            );

            all_matches.push((wants_entry.name, matched_cards));
        }

        let output = match self.output_format {
            OutputFormat::Regular => format_regular_output(&all_matches),
            OutputFormat::PickingList => {
                let all_cards: Vec<_> = all_matches.iter()
                    .flat_map(|(_, cards)| cards.iter().cloned())
                    .collect();
                format_picking_list(&all_cards)
            },
            OutputFormat::InvoiceList => {
                let all_cards: Vec<_> = all_matches.iter()
                    .flat_map(|(_, cards)| cards.iter().cloned())
                    .collect();
                format_invoice_list(&all_cards)
            },
            OutputFormat::UpdateStock => {
                let all_cards: Vec<_> = all_matches.iter()
                    .flat_map(|(_, cards)| cards.iter().cloned())
                    .collect();
                format_update_stock_csv(&all_cards)
            }
        };

        Ok(output)
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