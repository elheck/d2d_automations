use crate::{
    api::inventory_sync::{InventorySyncClient, PriceField, PriceFields},
    card_matching::MatchedCard,
    formatters::format_update_stock_csv,
    io::read_csv,
    price_trends::roc_from_history,
    ui::{
        components::{FilePicker, InventorySyncBar},
        screens::PickingState,
        state::{AppState, Screen, SearchAction, SearchState, SelectedSearchCard},
        style,
    },
};
use eframe::egui;
use log::{debug, error, info};
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
            if style::back_button(ui, "Back") {
                app_state.current_screen = Screen::Welcome;
            }
            ui.add_space(8.0);

            style::screen_heading(ui, "Card Search");

            // ── File picker ─────────────────────────────────────────────────
            style::section_frame().show(ui, |ui| {
                let browsed = FilePicker::new("CSV File:", &mut state.csv_path)
                    .with_filter("CSV", &["csv"])
                    .show(ui);
                ui.add_space(6.0);
                if (style::primary_button(ui, "Load CSV").clicked() || browsed)
                    && !state.csv_path.is_empty()
                {
                    Self::load_csv(app_state, state);
                }
            });

            ui.add_space(6.0);

            // ── Inventory Sync (enables the per-card price-history windows) ──
            InventorySyncBar::show(ui, ctx, app_state, |_, _| {});
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
                Self::show_search_results(ui, app_state, state);
            }
        });

        Self::show_history_window(ctx, state);
    }

    fn show_search_controls(ui: &mut egui::Ui, state: &mut SearchState) {
        style::section_frame().show(ui, |ui| {
            ui.label(
                egui::RichText::new("Search Settings")
                    .strong()
                    .color(style::TEXT_PRIMARY),
            );
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

                // Mode selector: send to lists, or write off as discarded stock.
                ui.horizontal(|ui| {
                    ui.label("Action:");
                    ui.radio_value(
                        &mut state.action_mode,
                        SearchAction::AddToLists,
                        "Add to lists",
                    );
                    ui.radio_value(
                        &mut state.action_mode,
                        SearchAction::Discard,
                        "Discard (remove without affecting revenue)",
                    );
                });

                ui.add_space(5.0);
                match state.action_mode {
                    SearchAction::AddToLists => {
                        ui.horizontal(|ui| {
                            if style::primary_button(ui, "Proceed to Lists").clicked() {
                                Self::proceed_to_lists(app_state, state, picking_state);
                            }
                            if style::secondary_button(ui, "Clear All").clicked() {
                                state.selected_cards.clear();
                            }
                        });
                    }
                    SearchAction::Discard => {
                        ui.label(
                            egui::RichText::new(
                                "Reduces inventory and exports a negative-delta stock-update CSV. \
                                 Discarded copies are NOT counted as sold. Import the CSV into \
                                 Cardmarket before your next inventory sync.",
                            )
                            .color(style::TEXT_MUTED),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if style::primary_button(ui, "Discard & Export CSV…").clicked() {
                                Self::perform_discard(state);
                            }
                            if style::secondary_button(ui, "Clear All").clicked() {
                                state.selected_cards.clear();
                            }
                        });
                    }
                }
            });
    }

    /// Writes off the selected cards as discarded stock: exports a negative-delta
    /// stock-update CSV (the save dialog acts as the confirmation gate), reduces the
    /// inventory DB without touching `sold_quantity`, decrements the in-memory
    /// quantities so the results reflect the write-off, and clears the selection.
    fn perform_discard(state: &mut SearchState) {
        if state.selected_cards.is_empty() {
            return;
        }

        // Build the negative-delta CSV from the current selection. The exporter
        // negates quantities, so pass the copies as-is.
        let matched: Vec<MatchedCard> = state
            .selected_cards
            .iter()
            .map(|sc| MatchedCard {
                card: &sc.card,
                quantity: sc.quantity,
                set_name: sc.card.set.clone(),
            })
            .collect();
        let csv = format_update_stock_csv(&matched);

        let Some(path) = rfd::FileDialog::new()
            .set_file_name("discarded_cards.csv")
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            // Cancelled — abort without altering the DB or the selection.
            info!("Discard cancelled: no export file chosen");
            return;
        };

        if let Err(e) = std::fs::write(&path, csv) {
            error!("Failed to save discard CSV: {e}");
            return;
        }

        // Apply the write-off to the inventory DB (revenue unaffected).
        let discards: Vec<(crate::models::Card, i64)> = state
            .selected_cards
            .iter()
            .map(|sc| (sc.card.clone(), sc.quantity as i64))
            .collect();
        match crate::inventory_db::discard_cards(&discards) {
            Ok(stats) => info!(
                "Discarded {} copies across {} variants",
                stats.copies_discarded, stats.variants_updated
            ),
            Err(e) => log::warn!("Inventory DB discard failed: {e}"),
        }

        // Reflect the reduced stock in the loaded card lists so the results and
        // remaining-stock caps stay accurate without a reload. Match on the full
        // variant identity (not just the shared product id) and reduce the first
        // matching row in each list.
        let same_variant = |a: &crate::models::Card, b: &crate::models::Card| {
            a.cardmarket_id == b.cardmarket_id
                && a.condition == b.condition
                && a.language == b.language
                && a.is_foil == b.is_foil
                && a.is_signed == b.is_signed
        };
        for sc in &state.selected_cards {
            for list in [&mut state.cards, &mut state.filtered_cards] {
                if let Some(card) = list.iter_mut().find(|c| same_variant(c, &sc.card)) {
                    let current: i32 = card.quantity.parse().unwrap_or(0);
                    card.quantity = (current - sc.quantity).max(0).to_string();
                }
            }
        }

        state.selected_cards.clear();
        state.quantity_inputs.clear();
    }

    fn show_search_results(ui: &mut egui::Ui, app_state: &AppState, state: &mut SearchState) {
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

                if style::secondary_button_enabled(ui, "⏮", state.current_page > 0).clicked() {
                    state.current_page = 0;
                }
                if style::secondary_button_enabled(ui, "⏪", state.current_page > 0).clicked() {
                    state.current_page -= 1;
                }

                ui.label(
                    egui::RichText::new(format!(
                        "Page {} of {}",
                        state.current_page + 1,
                        total_pages
                    ))
                    .color(style::TEXT_MUTED),
                );

                if style::secondary_button_enabled(ui, "⏩", state.current_page < total_pages - 1)
                    .clicked()
                {
                    state.current_page += 1;
                }
                if style::secondary_button_enabled(ui, "⏭", state.current_page < total_pages - 1)
                    .clicked()
                {
                    state.current_page = total_pages - 1;
                }
            });
        }

        ui.add_space(5.0);

        // Collect actions to apply after the grid (avoids borrow conflicts)
        let mut add_actions: Vec<(usize, i32)> = Vec::new();
        let mut history_action: Option<usize> = None;

        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 20.0)
            .show(ui, |ui| {
                egui::Grid::new("search_results")
                    .num_columns(11)
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
                        ui.strong("");
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
                            ui.label(format!("{}€", card.price));
                            ui.label(card.location.as_deref().unwrap_or(""));
                            ui.label(&card.rarity);

                            // Price-history window (needs the inventory_sync server)
                            if ui
                                .add(egui::Button::new("📈").small())
                                .on_hover_text("Price history from inventory_sync")
                                .clicked()
                            {
                                history_action = Some(abs_idx);
                            }

                            ui.end_row();
                        }
                    });
            });

        if let Some(abs_idx) = history_action {
            if let Some(card) = state.filtered_cards.get(abs_idx) {
                let card = card.clone();
                Self::spawn_history_fetch(state, &app_state.inventory_sync_url, &card);
            }
        }

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

    // ── Per-card price history ──────────────────────────────────────────────

    /// Days of history to chart in the per-card window.
    const HISTORY_DAYS: u32 = 120;

    /// Kicks off a background `GET /api/prices/{id}` fetch for one card.
    fn spawn_history_fetch(state: &mut SearchState, url: &str, card: &crate::models::Card) {
        let Ok(id) = card.cardmarket_id.parse::<u64>() else {
            state.history.open = true;
            state.history.title = card.name.clone();
            state.history.error = Some("Card has no usable cardmarket ID.".to_string());
            state.history.data = None;
            state.history.loading = false;
            return;
        };
        info!("Search: fetching price history for product {id} from {url}");
        state.history.open = true;
        state.history.title = card.name.clone();
        state.history.is_foil = card.is_foil_card();
        state.history.error = None;
        state.history.data = None;
        state.history.loading = true;
        let (tx, rx) = std::sync::mpsc::channel();
        state.history.rx = Some(rx);
        let client = InventorySyncClient::new(url);
        std::thread::spawn(move || {
            let result = client
                .price_history_blocking(id, Some(Self::HISTORY_DAYS))
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }

    /// Polls the fetch channel and renders the floating history window.
    fn show_history_window(ctx: &egui::Context, state: &mut SearchState) {
        if let Some(rx) = &state.history.rx {
            if let Ok(result) = rx.try_recv() {
                state.history.loading = false;
                state.history.rx = None;
                match result {
                    Ok(data) => state.history.data = Some(data),
                    Err(e) => state.history.error = Some(format!("History fetch failed: {e}")),
                }
            }
        }
        if state.history.loading {
            ctx.request_repaint();
        }
        if !state.history.open {
            return;
        }

        let mut open = state.history.open;
        let title = if state.history.is_foil {
            format!("Price history — {} ✦", state.history.title)
        } else {
            format!("Price history — {}", state.history.title)
        };
        egui::Window::new(title)
            .id(egui::Id::new("card_history_window"))
            .open(&mut open)
            .default_width(470.0)
            .show(ctx, |ui| {
                if state.history.loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Fetching history…");
                    });
                    return;
                }
                if let Some(err) = &state.history.error {
                    style::status_error(ui, err);
                    return;
                }
                let Some(data) = &state.history.data else {
                    return;
                };

                if let Some(expansion) = &data.product.expansion_name {
                    ui.label(
                        egui::RichText::new(format!("{} · {}", data.product.name, expansion))
                            .color(style::TEXT_MUTED)
                            .size(11.0),
                    );
                    ui.add_space(4.0);
                }

                let is_foil = state.history.is_foil;
                let points: Vec<(&str, f64)> = data
                    .history
                    .iter()
                    .filter_map(|p| {
                        p.price_for(PriceField::Trend, is_foil)
                            .map(|v| (p.price_date.as_str(), v))
                    })
                    .collect();

                if points.len() < 2 {
                    ui.label(
                        egui::RichText::new("Not enough price history for this card yet.")
                            .color(style::TEXT_MUTED),
                    );
                    return;
                }

                Self::draw_sparkline(ui, &points);
                ui.add_space(6.0);

                // Stats row: current trend + 7/30-day movement, computed
                // locally from the raw history rows (foil-aware).
                let current = points.last().map(|(_, v)| *v);
                ui.horizontal(|ui| {
                    ui.label("Trend:");
                    ui.label(
                        egui::RichText::new(
                            current
                                .map(|v| format!("€{v:.2}"))
                                .unwrap_or_else(|| "—".to_string()),
                        )
                        .strong(),
                    );
                    ui.add_space(10.0);
                    ui.label("Δ7d:");
                    style::change_pct_label(ui, roc_from_history(&data.history, 7, is_foil));
                    ui.add_space(10.0);
                    ui.label("Δ30d:");
                    style::change_pct_label(ui, roc_from_history(&data.history, 30, is_foil));
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(format!("{} days shown", points.len()))
                            .color(style::TEXT_MUTED)
                            .size(11.0),
                    );
                });
            });
        state.history.open = open;
    }

    /// Draws the trend-price line chart into an allocated rect.
    fn draw_sparkline(ui: &mut egui::Ui, points: &[(&str, f64)]) {
        let desired = egui::vec2(ui.available_width().min(440.0), 120.0);
        let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, style::PANEL_BG);

        let values: Vec<f64> = points.iter().map(|(_, v)| *v).collect();
        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        // Flat series still needs a visible band to map into.
        let (min, max) = if (max - min).abs() < 1e-9 {
            (min - 0.5, max + 0.5)
        } else {
            (min, max)
        };

        let pad = 8.0;
        let inner = rect.shrink(pad);
        let n = points.len();
        let line: Vec<egui::Pos2> = values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let x = inner.left() + inner.width() * i as f32 / (n - 1) as f32;
                let t = ((v - min) / (max - min)) as f32;
                let y = inner.bottom() - inner.height() * t;
                egui::pos2(x, y)
            })
            .collect();
        painter.add(egui::Shape::line(
            line,
            egui::Stroke::new(1.5_f32, style::ACCENT),
        ));

        let label = |pos: egui::Pos2, align: egui::Align2, text: String| {
            painter.text(
                pos,
                align,
                text,
                egui::FontId::proportional(10.0),
                style::TEXT_MUTED,
            );
        };
        label(
            rect.left_top() + egui::vec2(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            format!("€{max:.2}"),
        );
        label(
            rect.left_bottom() + egui::vec2(4.0, -2.0),
            egui::Align2::LEFT_BOTTOM,
            format!("€{min:.2}"),
        );
        label(
            rect.right_bottom() + egui::vec2(-4.0, -2.0),
            egui::Align2::RIGHT_BOTTOM,
            format!(
                "{} → {}",
                points.first().map(|(d, _)| *d).unwrap_or_default(),
                points.last().map(|(d, _)| *d).unwrap_or_default()
            ),
        );
    }

    fn load_csv(app_state: &mut AppState, state: &mut SearchState) {
        info!("Loading CSV for search: {}", state.csv_path);
        match read_csv(&state.csv_path) {
            Ok(cards) => {
                info!("Loaded {} cards for searching", cards.len());
                app_state.sync_inventory_guarded(&cards);
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

        // Also match legacy translated names when present (older CSV exports).
        if state.search_in_all_languages
            && state.selected_fields.name
            && (check_field(true, &card.name_de)
                || check_field(true, &card.name_es)
                || check_field(true, &card.name_fr)
                || check_field(true, &card.name_it))
        {
            return true;
        }

        false
    }
}
