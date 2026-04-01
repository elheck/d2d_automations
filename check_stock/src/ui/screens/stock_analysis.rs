use crate::{
    inventory_db::{DbStats, LotBreakdown, OldestInStockEntry},
    io::read_csv,
    ui::{
        components::FilePicker,
        state::{Screen, StockAnalysisState},
        style,
    },
};
use eframe::egui;

pub struct StockAnalysisScreen;

impl StockAnalysisScreen {
    pub fn show(ctx: &egui::Context, current_screen: &mut Screen, state: &mut StockAnalysisState) {
        // Load stats once on first render (non-blocking, DB is local SQLite)
        if !state.stats_loaded {
            state.stats_loaded = true;
            Self::refresh_stats(state);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("stock_analysis_scroll")
                .show(ui, |ui| {
                    if style::back_button(ui, "Back") {
                        *current_screen = Screen::Welcome;
                    }
                    ui.add_space(8.0);

                    style::screen_heading(ui, "Stock Analysis");

                    // ── File picker ─────────────────────────────────────────
                    style::section_frame().show(ui, |ui| {
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
                    });

                    ui.add_space(10.0);

                    // ── Database stats panel ────────────────────────────────
                    if let Some(stats) = &state.db_stats {
                        Self::show_db_stats(ui, stats);
                    } else if let Some(err) = &state.db_stats_error {
                        style::status_error(ui, &format!("Stats error: {err}"));
                    }
                });
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
        style::section_frame().show(ui, |ui| {
            ui.label(
                egui::RichText::new("Database Overview")
                    .strong()
                    .size(14.0)
                    .color(style::TEXT_PRIMARY),
            );
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

            if !stats.lot_breakdown.is_empty() {
                ui.add_space(8.0);
                Self::show_lot_breakdown(ui, &stats.lot_breakdown);
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

    fn show_lot_breakdown(ui: &mut egui::Ui, lots: &[LotBreakdown]) {
        ui.label(
            egui::RichText::new("Revenue by Lot")
                .strong()
                .size(14.0)
                .color(style::TEXT_PRIMARY),
        );
        ui.add_space(4.0);

        egui::Grid::new("lot_breakdown")
            .num_columns(6)
            .spacing([12.0, 2.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Lot").strong());
                ui.label(egui::RichText::new("In Stock").strong());
                ui.label(egui::RichText::new("Copies").strong());
                ui.label(egui::RichText::new("Stock Value").strong());
                ui.label(egui::RichText::new("Sold").strong());
                ui.label(egui::RichText::new("Revenue").strong());
                ui.end_row();

                let mut total_stock_value = 0.0;
                let mut total_sold_copies: i64 = 0;
                let mut total_revenue = 0.0;

                for lot in lots {
                    ui.label(&lot.lot);
                    ui.label(lot.in_stock_listings.to_string());
                    ui.label(format!("×{}", lot.in_stock_copies));
                    ui.label(format!("€{:.2}", lot.in_stock_value));
                    ui.label(format!("×{}", lot.sold_copies));
                    ui.label(format!("€{:.2}", lot.sold_revenue));
                    ui.end_row();

                    total_stock_value += lot.in_stock_value;
                    total_sold_copies += lot.sold_copies;
                    total_revenue += lot.sold_revenue;
                }

                // Totals row
                ui.label(egui::RichText::new("Total").strong());
                ui.label("");
                ui.label("");
                ui.label(egui::RichText::new(format!("€{total_stock_value:.2}")).strong());
                ui.label(egui::RichText::new(format!("×{total_sold_copies}")).strong());
                ui.label(egui::RichText::new(format!("€{total_revenue:.2}")).strong());
                ui.end_row();
            });
    }
}
