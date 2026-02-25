use crate::{
    inventory_db::{DbStats, OldestInStockEntry},
    io::read_csv,
    stock_analysis::{format_stock_analysis_with_sort, SortOrder, StockAnalysis},
    ui::{
        components::FilePicker,
        state::{Screen, StockAnalysisState},
    },
};
use eframe::egui;
use log::info;

pub struct StockAnalysisScreen;

impl StockAnalysisScreen {
    pub fn show(ctx: &egui::Context, current_screen: &mut Screen, state: &mut StockAnalysisState) {
        // Load stats once on first render (non-blocking, DB is local SQLite)
        if !state.stats_loaded {
            state.stats_loaded = true;
            Self::refresh_stats(state);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("← Back to Welcome Screen").clicked() {
                    *current_screen = Screen::Welcome;
                }
            });
            ui.add_space(10.0);

            ui.heading("Stock Analysis");
            ui.add_space(10.0);

            if FilePicker::new("Inventory CSV:", &mut state.inventory_path)
                .with_filter("CSV", &["csv"])
                .show(ui)
            {
                if let Ok(inventory) = read_csv(&state.inventory_path) {
                    if let Err(e) = crate::inventory_db::sync_inventory(&inventory) {
                        log::warn!("Inventory DB sync failed: {}", e);
                    }
                }
                Self::refresh_stats(state);
            }

            ui.add_space(10.0);

            // Database stats panel
            if let Some(stats) = &state.db_stats {
                Self::show_db_stats(ui, stats);
                ui.add_space(10.0);
            } else if let Some(err) = &state.db_stats_error {
                ui.colored_label(egui::Color32::RED, format!("Stats error: {err}"));
                ui.add_space(10.0);
            }

            ui.separator();
            ui.add_space(8.0);

            ui.label(egui::RichText::new("Bin Capacity Analysis").strong());
            ui.add_space(6.0);

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

    fn refresh_stats(state: &mut StockAnalysisState) {
        match crate::inventory_db::get_db_stats() {
            Ok(stats) => {
                state.db_stats = Some(stats);
                state.db_stats_error = None;
            }
            Err(e) => {
                state.db_stats_error = Some(e.to_string());
            }
        }
    }

    fn show_db_stats(ui: &mut egui::Ui, stats: &DbStats) {
        ui.group(|ui| {
            ui.label(egui::RichText::new("Database Overview").strong().size(14.0));
            ui.add_space(6.0);

            // Summary: 4-column grid (label, value, label, value)
            egui::Grid::new("db_stats_summary")
                .num_columns(4)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Total Listings:").strong());
                    ui.label(stats.total_articles.to_string());
                    ui.label(egui::RichText::new("In Stock:").strong());
                    ui.label(stats.in_stock_articles.to_string());
                    ui.end_row();

                    ui.label(egui::RichText::new("Total Copies:").strong());
                    ui.label(stats.total_copies.to_string());
                    ui.label(egui::RichText::new("Total Value:").strong());
                    ui.label(format!("€{:.2}", stats.total_value));
                    ui.end_row();

                    ui.label(egui::RichText::new("Foils:").strong());
                    ui.label(stats.foil_count.to_string());
                    ui.label(egui::RichText::new("Signed:").strong());
                    ui.label(stats.signed_count.to_string());
                    ui.end_row();

                    if let Some(date) = &stats.first_synced_date {
                        ui.label(egui::RichText::new("In stock since:").strong());
                        ui.label(date);
                        ui.end_row();
                    }
                });

            ui.add_space(8.0);

            // Top cards: two columns side by side
            ui.columns(2, |cols| {
                cols[0].label(egui::RichText::new("Most Copies").strong());
                egui::Grid::new("top_by_quantity")
                    .num_columns(2)
                    .spacing([8.0, 2.0])
                    .show(&mut cols[0], |ui| {
                        for (name, count) in &stats.top_by_quantity {
                            ui.label(name);
                            ui.label(format!("×{count}"));
                            ui.end_row();
                        }
                    });

                cols[1].label(egui::RichText::new("Most Expensive").strong());
                egui::Grid::new("top_by_price")
                    .num_columns(2)
                    .spacing([8.0, 2.0])
                    .show(&mut cols[1], |ui| {
                        for (name, price) in &stats.top_by_price {
                            ui.label(name);
                            ui.label(format!("€{price:.2}"));
                            ui.end_row();
                        }
                    });
            });

            if !stats.top_oldest_in_stock.is_empty() {
                ui.add_space(6.0);
                Self::show_longest_unsold(ui, &stats.top_oldest_in_stock);
            }

            // Oldest / newest listed
            if stats.oldest_listed.is_some() || stats.newest_listed.is_some() {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if let Some((name, date)) = &stats.oldest_listed {
                        ui.label(egui::RichText::new("Oldest listed:").strong());
                        ui.label(format!("{name} ({date})"));
                    }
                    if let Some((name, date)) = &stats.newest_listed {
                        ui.add_space(20.0);
                        ui.label(egui::RichText::new("Newest listed:").strong());
                        ui.label(format!("{name} ({date})"));
                    }
                });
            }

            // Breakdowns
            ui.add_space(6.0);

            if !stats.language_breakdown.is_empty() {
                let text = stats
                    .language_breakdown
                    .iter()
                    .map(|(lang, count)| format!("{lang}: {count}"))
                    .collect::<Vec<_>>()
                    .join("   ");
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Languages:").strong());
                    ui.label(text);
                });
            }

            if !stats.condition_breakdown.is_empty() {
                let text = stats
                    .condition_breakdown
                    .iter()
                    .map(|(cond, count)| format!("{cond}: {count}"))
                    .collect::<Vec<_>>()
                    .join("   ");
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Conditions:").strong());
                    ui.label(text);
                });
            }

            if !stats.rarity_breakdown.is_empty() {
                let text = stats
                    .rarity_breakdown
                    .iter()
                    .map(|(rarity, count)| format!("{rarity}: {count}"))
                    .collect::<Vec<_>>()
                    .join("   ");
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Rarities:").strong());
                    ui.label(text);
                });
            }
        });
    }

    fn show_longest_unsold(ui: &mut egui::Ui, entries: &[OldestInStockEntry]) {
        ui.label(egui::RichText::new("Longest Unsold").strong());
        egui::Grid::new("top_oldest_in_stock")
            .num_columns(5)
            .spacing([12.0, 2.0])
            .show(ui, |ui| {
                // Header row
                ui.label(egui::RichText::new("Card").strong());
                ui.label(egui::RichText::new("Since").strong());
                ui.label(egui::RichText::new("Qty").strong());
                ui.label(egui::RichText::new("Price").strong());
                ui.label(egui::RichText::new("Location").strong());
                ui.end_row();

                for e in entries {
                    ui.label(&e.name);
                    ui.label(&e.date);
                    ui.label(format!("×{}", e.quantity));
                    ui.label(format!("€{:.2}", e.price));
                    ui.label(if e.location.is_empty() {
                        "—"
                    } else {
                        &e.location
                    });
                    ui.end_row();
                }
            });
    }

    fn analyze_stock(state: &mut StockAnalysisState) -> Result<(), Box<dyn std::error::Error>> {
        if state.inventory_path.is_empty() {
            return Err("Please select an inventory file".into());
        }

        info!(
            "Starting stock analysis with {} free slots threshold",
            state.free_slots
        );

        let inventory = read_csv(&state.inventory_path)?;
        let analyzer = StockAnalysis::new(inventory);
        let stats = analyzer.analyze_with_free_slots(state.free_slots);

        info!(
            "Found {} bins with {} or more free slots",
            stats.available_bins.len(),
            state.free_slots
        );

        state.output = format_stock_analysis_with_sort(&stats, state.sort_order);
        Ok(())
    }
}
