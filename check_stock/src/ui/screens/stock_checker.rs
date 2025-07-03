use eframe::egui;
use crate::{
    ui::{
        state::{AppState, Screen, OutputFormat},
        components::{FilePicker, OutputWindow},
        language::Language,
    },
    formatters::{format_regular_output, format_picking_list, format_invoice_list, format_update_stock_csv},
    io::{read_csv, read_wantslist},
    card_matching::{find_matching_cards, MatchedCard},
};

pub struct StockCheckerScreen;

impl StockCheckerScreen {
    pub fn show(ctx: &egui::Context, state: &mut AppState) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("← Back to Welcome Screen").clicked() {
                    state.current_screen = Screen::Welcome;
                }
            });
            ui.add_space(10.0);
            
            ui.heading("MTG Stock Checker");
            ui.add_space(10.0);

            // File pickers
            FilePicker::new("Inventory CSV:", &mut state.inventory_path)
                .with_filter("CSV", &["csv"])
                .show(ui);

            FilePicker::new("Wantslist:", &mut state.wantslist_path)
                .show(ui);

            // Language selection
            ui.horizontal(|ui| {
                ui.label("Preferred Language:");
                egui::ComboBox::new("language_selector", "")
                    .selected_text(state.preferred_language.as_str())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut state.preferred_language, Language::English, "English");
                        ui.selectable_value(&mut state.preferred_language, Language::German, "German");
                        ui.selectable_value(&mut state.preferred_language, Language::Spanish, "Spanish");
                        ui.selectable_value(&mut state.preferred_language, Language::French, "French");
                        ui.selectable_value(&mut state.preferred_language, Language::Italian, "Italian");
                    });
                ui.checkbox(&mut state.preferred_language_only, "Only show cards in preferred language");
            });

            ui.horizontal(|ui| {
                if ui.button("Check Stock").clicked() {
                    if let Err(e) = Self::check_stock(state) {
                        state.output = format!("Error: {e}");
                    }
                }
            });

            ui.separator();

            if state.show_selection || state.selection_mode {
                Self::show_selection_view(ui, state);
            } else if !state.output.is_empty() {
                Self::show_regular_output(ui, state);
            }

            if state.show_output_window {
                Self::show_output_window(ctx, state);
            }
        });
    }

    fn check_stock(state: &mut AppState) -> Result<(), Box<dyn std::error::Error>> {
        if state.inventory_path.is_empty() || state.wantslist_path.is_empty() {
            return Err("Please select both inventory and wantslist files".into());
        }

        let inventory = read_csv(&state.inventory_path)?;
        let wantslist = read_wantslist(&state.wantslist_path)?;
        state.all_matches.clear();
        
        for wants_entry in wantslist {
            let mut matched_cards = find_matching_cards(
                &wants_entry.name,
                wants_entry.quantity,
                &inventory,
                Some(state.preferred_language.code())
            );

            if state.preferred_language_only {
                matched_cards.retain(|mc| {
                    mc.card.language.eq_ignore_ascii_case(state.preferred_language.as_str()) ||
                    (state.preferred_language == Language::German && mc.card.language == "German") ||
                    (state.preferred_language == Language::French && mc.card.language == "French") ||
                    (state.preferred_language == Language::Spanish && mc.card.language == "Spanish") ||
                    (state.preferred_language == Language::Italian && mc.card.language == "Italian")
                });
            }
            
            let owned_cards = matched_cards
                .into_iter()
                .map(|mc| {
                    let card = (*mc.card).clone();
                    (card, mc.quantity, mc.set_name)
                })
                .collect();
                
            state.all_matches.push((wants_entry.name, owned_cards));
        }

        Self::generate_regular_output(state);
        Ok(())
    }

    fn show_selection_view(ui: &mut egui::Ui, state: &mut AppState) {
        ui.label("Select the cards you want to include:");
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 50.0)
            .show(ui, |ui| {
                let mut idx = 0;
                for (card_name, cards) in &state.all_matches {
                    if !cards.is_empty() {
                        ui.label(format!("{card_name}:"));
                        for (card, quantity, set_name) in cards {
                            let mut checked = state.selected[idx];
                            let location_info = card.location.as_ref()
                                .filter(|loc| !loc.trim().is_empty())
                                .map(|loc| format!(" [Location: {loc}]"))
                                .unwrap_or_default();
                                
                            let label = format!(
                                "{} {} [{}] from {} - {} condition - {:.2} €{}",
                                quantity,
                                card.name,
                                card.language,
                                set_name,
                                card.condition,
                                card.price.parse::<f64>().unwrap_or(0.0),
                                location_info
                            );
                            if ui.checkbox(&mut checked, label).changed() {
                                state.selected[idx] = checked;
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
                    Self::generate_selected_output(state, OutputFormat::PickingList);
                }
                if ui.button("Generate Invoice List").clicked() {
                    Self::generate_selected_output(state, OutputFormat::InvoiceList);
                }
                if ui.button("Generate Stock Update CSV").clicked() {
                    Self::generate_selected_output(state, OutputFormat::UpdateStock);
                }
                if ui.button("Return to Regular List").clicked() {
                    state.show_selection = false;
                    state.selection_mode = false;
                    Self::generate_regular_output(state);
                }
            });
        });
    }

    fn show_regular_output(ui: &mut egui::Ui, state: &mut AppState) {
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 50.0)
            .show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut state.output)
                    .desired_width(f32::INFINITY)
                    .desired_rows(20)
                    .font(egui::TextStyle::Monospace));
            });

        if !state.all_matches.is_empty() {
            ui.separator();
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Select Cards for Lists").clicked() {
                        Self::start_selection(state);
                    }
                });
            });
        }
    }

    fn show_output_window(ctx: &egui::Context, state: &mut AppState) {
        let extension = if state.output_window_title == "Stock Update" { "csv" } else { "txt" };
        OutputWindow::new(
            &state.output_window_title,
            &mut state.output_window_content,
            &mut state.show_output_window,
            extension,
        ).show(ctx);
    }

    fn start_selection(state: &mut AppState) {
        if state.selected.is_empty() {              state.selected = std::iter::repeat_n(true, state.all_matches.iter().map(|(_, cards)| cards.len()).sum())
                .collect();
        }
        state.show_selection = true;
        state.selection_mode = true;
    }

    fn generate_regular_output(state: &mut AppState) {
        let selected_matches: Vec<_> = state.all_matches.iter()
            .map(|(name, cards)| {
                let group_cards: Vec<_> = cards.iter()
                    .map(|(card, quantity, set_name)| MatchedCard {
                        card,
                        quantity: *quantity,
                        set_name: set_name.clone(),
                    })
                    .collect();
                (name.clone(), group_cards)
            })
            .collect();
        
        state.output = format_regular_output(&selected_matches);
    }

    fn generate_selected_output(state: &mut AppState, format: OutputFormat) {
        let mut selected_matches = Vec::new();
        let mut idx = 0;
        
        for (name, cards) in &state.all_matches {
            let mut group_cards = Vec::new();
            for (card, quantity, set_name) in cards {
                if state.selected[idx] {
                    group_cards.push(MatchedCard {
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

        state.output_window_content = match format {
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

        state.output_window_title = format.title().to_string();
        state.show_output_window = true;
    }
}