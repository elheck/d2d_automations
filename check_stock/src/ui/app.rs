use eframe::{self, egui};
use egui::ViewportBuilder;

use super::{
    screens::{StockAnalysisScreen, StockCheckerScreen, WelcomeScreen},
    state::{AppState, Screen, StockAnalysisState},
};

#[derive(Default)]
pub struct StockCheckerApp {
    app_state: AppState,
    analysis_state: StockAnalysisState,
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
                StockCheckerScreen::show(ctx, &mut self.app_state);
            }
            Screen::StockAnalysis => {
                StockAnalysisScreen::show(
                    ctx,
                    &mut self.app_state.current_screen,
                    &mut self.analysis_state,
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
