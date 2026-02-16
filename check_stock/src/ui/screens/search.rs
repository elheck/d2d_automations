use crate::{
    io::read_csv,
    ui::{
        components::FilePicker,
        screens::PickingState,
        state::{AppState, Screen, SearchState, SelectedSearchCard},
    },
};
use eframe::egui;
use log::{debug, info};
use std::collections::HashMap;
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

    pub fn show(
        ctx: &egui::Context,
        app_state: &mut AppState,
        state: &mut SearchState,
        picking_state: &mut PickingState,
    ) {
        // Check if we need to perform a delayed search
        Self::check_delayed_search(state);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("← Back to Menu").clicked() {
                    app_state.current_screen = Screen::Welcome;
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

                // Selected cards panel
                Self::show_selected_cards_panel(ui, app_state, state, picking_state);
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

    fn show_selected_cards_panel(
        ui: &mut egui::Ui,
        app_state: &mut AppState,
        state: &mut SearchState,
        picking_state: &mut PickingState,
    ) {
        if state.selected_cards.is_empty() {
            return;
        }

        let total_price: f64 = state
            .selected_cards
            .iter()
            .map(|sc| sc.card.price.parse::<f64>().unwrap_or(0.0) * sc.quantity as f64)
            .sum();

        let card_count: i32 = state.selected_cards.iter().map(|sc| sc.quantity).sum();

        let header = format!("Selected Cards ({card_count})  —  Total: {total_price:.2} €");
        egui::CollapsingHeader::new(header)
            .default_open(true)
            .show(ui, |ui| {
                let mut remove_idx = None;

                egui::ScrollArea::vertical()
                    .max_height(150.0)
                    .id_salt("selected_cards_scroll")
                    .show(ui, |ui| {
                        for (i, sc) in state.selected_cards.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let price = sc.card.price.parse::<f64>().unwrap_or(0.0);
                                ui.label(format!(
                                    "{}x {} [{}] {} - {:.2} €",
                                    sc.quantity, sc.card.name, sc.card.language, sc.card.set, price
                                ));
                                if ui.small_button("✕").clicked() {
                                    remove_idx = Some(i);
                                }
                            });
                        }
                    });

                if let Some(idx) = remove_idx {
                    state.selected_cards.remove(idx);
                }

                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    if ui.button("Proceed to Lists").clicked() {
                        Self::proceed_to_lists(app_state, state, picking_state);
                    }
                    if ui.button("Clear All").clicked() {
                        state.selected_cards.clear();
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

        // Collect add actions to apply after the grid (avoids borrow conflicts)
        let mut add_actions: Vec<(usize, i32)> = Vec::new();

        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 20.0)
            .show(ui, |ui| {
                egui::Grid::new("search_results")
                    .num_columns(10)
                    .spacing([10.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        // Header
                        ui.strong("");
                        ui.strong("Qty");
                        ui.strong("Stock");
                        ui.strong("Name");
                        ui.strong("Set");
                        ui.strong("Language");
                        ui.strong("Condition");
                        ui.strong("Price");
                        ui.strong("Location");
                        ui.strong("Rarity");
                        ui.end_row();

                        // Results - only show the current page
                        for (rel_idx, card) in
                            state.filtered_cards[start_idx..end_idx].iter().enumerate()
                        {
                            let abs_idx = start_idx + rel_idx;
                            let available: i32 = card.quantity.parse().unwrap_or(1).max(1);

                            // Already selected quantity for this card
                            let already_selected: i32 = state
                                .selected_cards
                                .iter()
                                .find(|sc| sc.card.cardmarket_id == card.cardmarket_id)
                                .map(|sc| sc.quantity)
                                .unwrap_or(0);
                            let remaining = (available - already_selected).max(0);

                            // Add button (disabled when no remaining stock)
                            let add_btn = ui.add_enabled(remaining > 0, egui::Button::new("Add"));
                            if add_btn.clicked() {
                                let qty = state.quantity_inputs.entry(abs_idx).or_insert(1);
                                add_actions.push((abs_idx, *qty));
                            }

                            // Quantity input (capped to remaining stock)
                            let qty = state.quantity_inputs.entry(abs_idx).or_insert(1);
                            if *qty > remaining {
                                *qty = remaining.max(1);
                            }
                            ui.add(
                                egui::DragValue::new(qty)
                                    .range(1..=remaining.max(1))
                                    .speed(0.1),
                            );

                            // Stock count right next to qty
                            ui.label(&card.quantity);

                            ui.label(&card.name);
                            ui.label(&card.set);
                            ui.label(&card.language);
                            ui.label(&card.condition);
                            ui.label(format!("{}€", &card.price));
                            ui.label(card.location.as_deref().unwrap_or(""));
                            ui.label(&card.rarity);

                            ui.end_row();
                        }
                    });
            });

        // Apply add actions (capped to available stock)
        for (abs_idx, qty) in add_actions {
            if let Some(card) = state.filtered_cards.get(abs_idx) {
                let available: i32 = card.quantity.parse().unwrap_or(1).max(1);
                if let Some(existing) = state
                    .selected_cards
                    .iter_mut()
                    .find(|sc| sc.card.cardmarket_id == card.cardmarket_id)
                {
                    let remaining = available - existing.quantity;
                    if remaining > 0 {
                        existing.quantity += qty.min(remaining);
                    }
                } else {
                    state.selected_cards.push(SelectedSearchCard {
                        card: card.clone(),
                        quantity: qty.min(available),
                    });
                }
            }
        }
    }

    fn proceed_to_lists(
        app_state: &mut AppState,
        state: &mut SearchState,
        _picking_state: &mut PickingState,
    ) {
        // Group selected cards by name
        let mut groups: HashMap<String, Vec<SelectedSearchCard>> = HashMap::new();
        for sc in &state.selected_cards {
            groups
                .entry(sc.card.name.clone())
                .or_default()
                .push(SelectedSearchCard {
                    card: sc.card.clone(),
                    quantity: sc.quantity,
                });
        }

        // Convert to AppState.all_matches format
        app_state.all_matches.clear();
        app_state.selected.clear();

        for (name, cards) in &groups {
            let needed_qty: i32 = cards.iter().map(|sc| sc.quantity).sum();
            let match_tuples: Vec<_> = cards
                .iter()
                .map(|sc| {
                    let set_name = format!("{} ({})", sc.card.set, sc.card.set_code);
                    (sc.card.clone(), sc.quantity, set_name)
                })
                .collect();
            app_state
                .all_matches
                .push((name.clone(), needed_qty, match_tuples));
        }

        // All pre-selected
        let total_cards: usize = app_state
            .all_matches
            .iter()
            .map(|(_, _, cards)| cards.len())
            .sum();
        app_state.selected = vec![true; total_cards];
        app_state.show_selection = true;
        app_state.selection_mode = true;
        app_state.current_screen = Screen::StockChecker;

        info!(
            "Proceeding to lists with {} card groups ({} total entries)",
            groups.len(),
            total_cards
        );
    }

    fn load_csv(state: &mut SearchState) {
        info!("Loading CSV for search: {}", state.csv_path);
        match read_csv(&state.csv_path) {
            Ok(cards) => {
                info!("Loaded {} cards for searching", cards.len());
                state.cards = cards.clone();
                state.filtered_cards = cards;
                state.quantity_inputs.clear();
                if !state.search_term.is_empty() {
                    Self::perform_search(state);
                }
            }
            Err(e) => {
                log::error!("Error loading CSV: {}", e);
                eprintln!("Error loading CSV: {}", e);
            }
        }
    }

    fn perform_search(state: &mut SearchState) {
        if state.search_term.is_empty() {
            state.filtered_cards = state.cards.clone();
            state.current_page = 0; // Reset to first page
            state.quantity_inputs.clear();
            return;
        }

        debug!("Performing search for: '{}'", state.search_term);

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
        state.quantity_inputs.clear();
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
