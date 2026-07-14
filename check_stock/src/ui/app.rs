use eframe::{self, egui};
use egui::ViewportBuilder;

use super::{
    screens::{
        BinAnalysisScreen, BuyHelperScreen, ConsolidationScreen, ConsolidationState,
        MispricingScreen, PickingScreen, PickingState, PricingScreen, RestockScreen, SearchScreen,
        StockAnalysisScreen, StockCheckerScreen, StockListingScreen, WelcomeScreen,
    },
    state::{
        AppState, BinAnalysisState, BuyHelperState, MispricingState, PricingState, RestockState,
        Screen, SearchState, StockAnalysisState, StockListingState,
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
    consolidation_state: ConsolidationState,
    restock_state: RestockState,
}

impl eframe::App for StockCheckerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        match self.app_state.current_screen {
            Screen::Welcome => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    WelcomeScreen::show(ui, &mut self.app_state.current_screen);
                });
            }
            Screen::StockChecker => {
                StockCheckerScreen::show(ctx, &mut self.app_state, &mut self.picking_state);
            }
            Screen::StockAnalysis => {
                StockAnalysisScreen::show(
                    ctx,
                    &mut self.app_state.current_screen,
                    &mut self.analysis_state,
                );
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
                MispricingScreen::show(
                    ctx,
                    &mut self.app_state.current_screen,
                    &mut self.mispricing_state,
                );
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
