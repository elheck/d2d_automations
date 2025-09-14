use crate::{
    io::read_csv,
    ui::{
        components::FilePicker,
        state::{Screen, SearchState},
    },
};
use eframe::egui;
use std::time::Instant;

pub struct SearchScreen;

impl SearchScreen {
    const SEARCH_DEBOUNCE_MS: u64 = 300; // Wait 300ms after user stops typing

    fn check_delayed_search(state: &mut SearchState) {
        if state.search_needs_update {
            // Check if enough time has passed since the last change
            if state.last_search_time.elapsed().as_millis() >= Self::SEARCH_DEBOUNCE_MS as u128 {
                // Only perform search if the term has actually changed
                if state.search_term != state.last_search_term {
                    Self::perform_search(state);
                    state.last_search_term = state.search_term.clone();
                }
                state.search_needs_update = false;
            }
        }
    }

    pub fn show(ctx: &egui::Context, current_screen: &mut Screen, state: &mut SearchState) {
        // Check if we need to perform a delayed search
        Self::check_delayed_search(state);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("← Back to Menu").clicked() {
                    *current_screen = Screen::Welcome;
                }
            });
            ui.add_space(10.0);

            ui.heading("Card Search");
            ui.add_space(10.0);

            // File picker for CSV
            FilePicker::new("CSV File:", &mut state.csv_path)
                .with_filter("CSV", &["csv"])
                .show(ui);

            ui.add_space(10.0);

            // Load CSV button
            if ui.button("Load CSV").clicked() && !state.csv_path.is_empty() {
                Self::load_csv(state);
            }

            ui.add_space(10.0);

            if !state.cards.is_empty() {
                ui.label(format!("Loaded {} cards", state.cards.len()));
                ui.add_space(10.0);

                // Search controls
                Self::show_search_controls(ui, state);
                ui.add_space(10.0);

                // Results
                Self::show_search_results(ui, state);
            }
        });
    }

    fn show_search_controls(ui: &mut egui::Ui, state: &mut SearchState) {
        ui.group(|ui| {
            ui.label("Search Settings:");
            ui.add_space(5.0);

            // Search term input
            ui.horizontal(|ui| {
                ui.label("Search:");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut state.search_term)
                        .desired_width(300.0)
                        .hint_text("Enter search term..."),
                );

                if response.changed() {
                    // Mark that we need to update search and reset the timer
                    state.search_needs_update = true;
                    state.last_search_time = Instant::now();
                }

                if ui.button("Clear").clicked() {
                    state.search_term.clear();
                    state.last_search_term.clear();
                    state.filtered_cards = state.cards.clone();
                    state.search_needs_update = false;
                }
            });

            ui.add_space(5.0);

            // Search options
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut state.search_case_sensitive, "Case sensitive")
                    .changed()
                {
                    Self::perform_search(state);
                }

                if ui
                    .checkbox(&mut state.search_in_all_languages, "Search all languages")
                    .changed()
                {
                    Self::perform_search(state);
                }
            });

            ui.add_space(5.0);

            // Field selection
            ui.label("Search in fields:");
            ui.horizontal_wrapped(|ui| {
                let mut search_fields_changed = false;

                search_fields_changed |= ui
                    .checkbox(&mut state.selected_fields.name, "Name")
                    .changed();
                search_fields_changed |=
                    ui.checkbox(&mut state.selected_fields.set, "Set").changed();
                search_fields_changed |= ui
                    .checkbox(&mut state.selected_fields.condition, "Condition")
                    .changed();
                search_fields_changed |= ui
                    .checkbox(&mut state.selected_fields.language, "Language")
                    .changed();
                search_fields_changed |= ui
                    .checkbox(&mut state.selected_fields.location, "Location")
                    .changed();
                search_fields_changed |= ui
                    .checkbox(&mut state.selected_fields.rarity, "Rarity")
                    .changed();
                search_fields_changed |= ui
                    .checkbox(&mut state.selected_fields.price, "Price")
                    .changed();
                search_fields_changed |= ui
                    .checkbox(&mut state.selected_fields.comment, "Comment")
                    .changed();

                if state.search_in_all_languages {
                    search_fields_changed |= ui
                        .checkbox(&mut state.selected_fields.name_de, "German")
                        .changed();
                    search_fields_changed |= ui
                        .checkbox(&mut state.selected_fields.name_es, "Spanish")
                        .changed();
                    search_fields_changed |= ui
                        .checkbox(&mut state.selected_fields.name_fr, "French")
                        .changed();
                    search_fields_changed |= ui
                        .checkbox(&mut state.selected_fields.name_it, "Italian")
                        .changed();
                }

                if search_fields_changed {
                    Self::perform_search(state);
                }
            });
        });
    }

    fn show_search_results(ui: &mut egui::Ui, state: &mut SearchState) {
        let total_results = state.filtered_cards.len();
        let total_pages = total_results.div_ceil(state.results_per_page);

        // Ensure current page is valid
        if state.current_page >= total_pages && total_pages > 0 {
            state.current_page = total_pages - 1;
        }

        // Calculate the range of results to display
        let start_idx = state.current_page * state.results_per_page;
        let end_idx = std::cmp::min(start_idx + state.results_per_page, total_results);

        ui.horizontal(|ui| {
            ui.label(format!(
                "Found {} cards (showing {}-{} of {})",
                total_results,
                if total_results > 0 { start_idx + 1 } else { 0 },
                end_idx,
                total_results
            ));
        });

        // Pagination controls
        if total_pages > 1 {
            ui.horizontal(|ui| {
                ui.label("Results per page:");
                egui::ComboBox::from_id_salt("results_per_page")
                    .selected_text(format!("{}", state.results_per_page))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut state.results_per_page, 50, "50");
                        ui.selectable_value(&mut state.results_per_page, 100, "100");
                        ui.selectable_value(&mut state.results_per_page, 200, "200");
                        ui.selectable_value(&mut state.results_per_page, 500, "500");
                    });

                ui.add_space(20.0);

                if ui.button("⏮ First").clicked() && state.current_page > 0 {
                    state.current_page = 0;
                }

                if ui.button("⏪ Previous").clicked() && state.current_page > 0 {
                    state.current_page -= 1;
                }

                ui.label(format!(
                    "Page {} of {}",
                    state.current_page + 1,
                    total_pages
                ));

                if ui.button("Next ⏩").clicked() && state.current_page < total_pages - 1 {
                    state.current_page += 1;
                }

                if ui.button("Last ⏭").clicked() && state.current_page < total_pages - 1 {
                    state.current_page = total_pages - 1;
                }
            });
        }

        ui.add_space(5.0);

        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 20.0)
            .show(ui, |ui| {
                egui::Grid::new("search_results")
                    .num_columns(8)
                    .spacing([10.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        // Header
                        ui.strong("Name");
                        ui.strong("Set");
                        ui.strong("Language");
                        ui.strong("Condition");
                        ui.strong("Price");
                        ui.strong("Quantity");
                        ui.strong("Location");
                        ui.strong("Rarity");
                        ui.end_row();

                        // Results - only show the current page
                        for card in &state.filtered_cards[start_idx..end_idx] {
                            ui.label(&card.name);
                            ui.label(&card.set);
                            ui.label(&card.language);
                            ui.label(&card.condition);
                            ui.label(format!("{}€", &card.price));
                            ui.label(&card.quantity);
                            ui.label(card.location.as_deref().unwrap_or(""));
                            ui.label(&card.rarity);
                            ui.end_row();
                        }
                    });
            });
    }

    fn load_csv(state: &mut SearchState) {
        match read_csv(&state.csv_path) {
            Ok(cards) => {
                state.cards = cards.clone();
                state.filtered_cards = cards;
                if !state.search_term.is_empty() {
                    Self::perform_search(state);
                }
            }
            Err(e) => {
                eprintln!("Error loading CSV: {}", e);
            }
        }
    }

    fn perform_search(state: &mut SearchState) {
        if state.search_term.is_empty() {
            state.filtered_cards = state.cards.clone();
            state.current_page = 0; // Reset to first page
            return;
        }

        let search_term = if state.search_case_sensitive {
            state.search_term.clone()
        } else {
            state.search_term.to_lowercase()
        };

        state.filtered_cards = state
            .cards
            .iter()
            .filter(|card| Self::card_matches(card, &search_term, state))
            .cloned()
            .collect();

        state.current_page = 0; // Reset to first page on new search
    }

    fn card_matches(card: &crate::models::Card, search_term: &str, state: &SearchState) -> bool {
        // Helper function to check a field
        let check_field = |should_search: bool, field_value: &str| -> bool {
            if !should_search {
                return false;
            }
            let field_text = if state.search_case_sensitive {
                field_value.to_string()
            } else {
                field_value.to_lowercase()
            };
            field_text.contains(search_term)
        };

        // Check main string fields
        if check_field(state.selected_fields.name, &card.name)
            || check_field(state.selected_fields.set, &card.set)
            || check_field(state.selected_fields.condition, &card.condition)
            || check_field(state.selected_fields.language, &card.language)
            || check_field(state.selected_fields.rarity, &card.rarity)
            || check_field(state.selected_fields.price, &card.price)
            || check_field(state.selected_fields.comment, &card.comment)
        {
            return true;
        }

        // Check location (Option<String>)
        if state.selected_fields.location {
            if let Some(location) = &card.location {
                if check_field(true, location) {
                    return true;
                }
            }
        }

        // Check language-specific name fields
        if state.search_in_all_languages
            && (check_field(state.selected_fields.name_de, &card.name_de)
                || check_field(state.selected_fields.name_es, &card.name_es)
                || check_field(state.selected_fields.name_fr, &card.name_fr)
                || check_field(state.selected_fields.name_it, &card.name_it))
        {
            return true;
        }

        false
    }
}
