use eframe::egui;
use crate::ui::state::Screen;

pub struct WelcomeScreen;

impl WelcomeScreen {
    pub fn show(ui: &mut egui::Ui, current_screen: &mut Screen) {
        ui.vertical_centered(|ui| {
            ui.add_space(100.0);
            ui.heading("Welcome to D2D Automations");
            ui.add_space(20.0);
            
            if ui.button("Stock Checker").clicked() {
                *current_screen = Screen::StockChecker;
            }
            
            ui.add_space(10.0);
            
            if ui.button("Stock Analysis").clicked() {
                *current_screen = Screen::StockAnalysis;
            }
        });
    }
}