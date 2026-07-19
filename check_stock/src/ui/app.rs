use eframe::{self, egui};
use egui::ViewportBuilder;

use super::{
    screens::{
        BinAnalysisScreen, BuyHelperScreen, ConsolidationScreen, ConsolidationState,
        MispricingScreen, MoversScreen, PickingScreen, PickingState, PricingScreen, RestockScreen,
        SearchScreen, StockAnalysisScreen, StockCheckerScreen, StockListingScreen, WelcomeScreen,
    },
    state::{
        AppState, BinAnalysisState, BuyHelperState, MispricingState, MoversState, PricingState,
        RestockState, Screen, SearchState, StockAnalysisState, StockListingState,
    },
};

#[derive(Default)]
pub struct StockCheckerApp {
    app_state: AppState,
    analysis_state: StockAnalysisState,
    bin_analysis_state: BinAnalysisState,
    listing_state: StockListingState,
    search_state: SearchState,
    picking_state: PickingState,
    pricing_state: PricingState,
    buy_helper_state: BuyHelperState,
    mispricing_state: MispricingState,
    movers_state: MoversState,
    consolidation_state: ConsolidationState,
    restock_state: RestockState,
}

impl eframe::App for StockCheckerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        match self.app_state.current_screen {
            Screen::Welcome => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    WelcomeScreen::show(ui, &mut self.app_state);
                });
            }
            Screen::StockChecker => {
                StockCheckerScreen::show(ctx, &mut self.app_state, &mut self.picking_state);
            }
            Screen::StockAnalysis => {
                StockAnalysisScreen::show(ctx, &mut self.app_state, &mut self.analysis_state);
            }
            Screen::BinAnalysis => {
                BinAnalysisScreen::show(
                    ctx,
                    &mut self.app_state.current_screen,
                    &mut self.bin_analysis_state,
                    &mut self.consolidation_state,
                );
            }
            Screen::StockListing => {
                StockListingScreen::show(
                    ctx,
                    &mut self.app_state.current_screen,
                    &mut self.listing_state,
                );
            }
            Screen::Search => {
                SearchScreen::show(
                    ctx,
                    &mut self.app_state,
                    &mut self.search_state,
                    &mut self.picking_state,
                );
            }
            Screen::Picking => {
                PickingScreen::show(
                    ctx,
                    &mut self.app_state.current_screen,
                    &mut self.picking_state,
                );
            }
            Screen::Pricing => {
                PricingScreen::show(ctx, &mut self.app_state, &mut self.pricing_state);
            }
            Screen::BuyHelper => {
                BuyHelperScreen::show(
                    ctx,
                    &mut self.app_state.current_screen,
                    &mut self.buy_helper_state,
                );
            }
            Screen::Mispricing => {
                MispricingScreen::show(ctx, &mut self.app_state, &mut self.mispricing_state);
            }
            Screen::Movers => {
                MoversScreen::show(ctx, &mut self.app_state, &mut self.movers_state);
            }
            Screen::Consolidation => {
                ConsolidationScreen::show(
                    ctx,
                    &mut self.app_state.current_screen,
                    &mut self.consolidation_state,
                );
            }
            Screen::Restock => {
                RestockScreen::show(
                    ctx,
                    &mut self.app_state.current_screen,
                    &mut self.restock_state,
                );
            }
        }

        show_sync_guard_modal(ctx, &mut self.app_state);
    }
}

/// Modal shown (on any screen) when the import safety check blocked a CSV
/// sync. Nothing has been written yet; the user chooses to apply or drop it.
fn show_sync_guard_modal(ctx: &egui::Context, app_state: &mut AppState) {
    let Some(guard) = &app_state.sync_guard else {
        return;
    };
    let p = guard.preview.clone();
    let mut confirm = false;
    let mut cancel = false;
    egui::Window::new("⚠ Import safety check")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(
                "This inventory import would remove a large share of your stock. \
                 That usually means a truncated or wrong CSV — applying it would \
                 record the missing copies as sales.",
            );
            ui.add_space(6.0);
            egui::Grid::new("sync_guard_preview")
                .num_columns(2)
                .spacing([16.0, 2.0])
                .show(ui, |ui| {
                    ui.label("In-stock copies:");
                    ui.label(format!("{} → {}", p.copies_before, p.copies_after));
                    ui.end_row();
                    ui.label("Would be recorded as sold:");
                    ui.label(format!("{} copies", p.copies_sold));
                    ui.end_row();
                    ui.label("Variants zeroed out:");
                    ui.label(format!("{}", p.zeroed_variants));
                    ui.end_row();
                    ui.label("New variants:");
                    ui.label(format!("{}", p.new_variants));
                    ui.end_row();
                    ui.label("Price changes:");
                    ui.label(format!("{}", p.price_changes));
                    ui.end_row();
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel import (keep database)").clicked() {
                    cancel = true;
                }
                if ui
                    .button(egui::RichText::new("Sync anyway").strong())
                    .clicked()
                {
                    confirm = true;
                }
            });
            ui.label(
                egui::RichText::new("A dated backup of the database was taken before this check.")
                    .size(10.0)
                    .weak(),
            );
        });

    if confirm {
        if let Some(guard) = app_state.sync_guard.take() {
            match crate::inventory_db::sync_inventory_forced(&guard.cards) {
                Ok(stats) => log::info!(
                    "Forced inventory sync applied: {} upserted, {} zeroed",
                    stats.upserted,
                    stats.zeroed
                ),
                Err(e) => log::warn!("Forced inventory sync failed: {e}"),
            }
        }
    } else if cancel {
        app_state.sync_guard = None;
        log::info!("Blocked inventory sync dropped by user");
    }
}

pub fn launch_gui() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "MTG Stock Checker",
        options,
        Box::new(|_cc| Ok(Box::new(StockCheckerApp::default()))),
    )
}
