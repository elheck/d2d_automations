use crate::models::Card;
use super::language::Language;

type CardMatch = (Card, i32, String);
type CardMatchGroup = (String, Vec<CardMatch>);

#[derive(PartialEq)]
pub enum Screen {
    Welcome,
    StockChecker,
    StockAnalysis,
}

#[derive(PartialEq)]
pub enum OutputFormat {
    PickingList,
    InvoiceList,
    UpdateStock,
}

impl OutputFormat {
    pub fn title(&self) -> &'static str {
        match self {
            OutputFormat::PickingList => "Picking List",
            OutputFormat::InvoiceList => "Invoice List",
            OutputFormat::UpdateStock => "Stock Update",
        }
    }
}

pub struct AppState {
    pub discount_percent: f32, // 0.0 to 100.0
    pub current_screen: Screen,
    pub inventory_path: String,
    pub wantslist_path: String,
    pub output: String,
    pub preferred_language: Language,
    pub preferred_language_only: bool,
    pub all_matches: Vec<CardMatchGroup>,
    pub selected: Vec<bool>,
    pub show_selection: bool,
    pub selection_mode: bool,
    pub show_output_window: bool,
    pub output_window_content: String,
    pub output_window_title: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            current_screen: Screen::Welcome,
            inventory_path: String::new(),
            wantslist_path: String::new(),
            output: String::new(),
            preferred_language: Language::English,
            preferred_language_only: false,
            all_matches: Vec::new(),
            selected: Vec::new(),
            show_selection: false,
            selection_mode: false,
            show_output_window: false,
            output_window_content: String::new(),
            output_window_title: String::new(),
            discount_percent: 10.0,
        }
    }
}

pub struct StockAnalysisState {
    pub inventory_path: String,
    pub output: String,
    pub free_slots: i32,
}

impl Default for StockAnalysisState {
    fn default() -> Self {
        Self {
            inventory_path: String::new(),
            output: String::new(),
            free_slots: 5,
        }
    }
}