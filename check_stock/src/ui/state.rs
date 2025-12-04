use super::language::Language;
use crate::models::Card;
use crate::stock_analysis::SortOrder;

type CardMatch = (Card, i32, String);
/// (card_name, needed_quantity, matched_cards)
type CardMatchGroup = (String, i32, Vec<CardMatch>);

#[derive(PartialEq)]
pub enum Screen {
    Welcome,
    StockChecker,
    StockAnalysis,
    Search,
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
    pub sort_order: SortOrder,
}

impl Default for StockAnalysisState {
    fn default() -> Self {
        Self {
            inventory_path: String::new(),
            output: String::new(),
            free_slots: 5,
            sort_order: SortOrder::ByFreeSlots,
        }
    }
}

pub struct SearchState {
    pub csv_path: String,
    pub search_term: String,
    pub last_search_term: String,
    pub cards: Vec<Card>,
    pub filtered_cards: Vec<Card>,
    pub search_case_sensitive: bool,
    pub search_in_all_languages: bool,
    pub selected_fields: SearchFields,
    pub last_search_time: std::time::Instant,
    pub search_needs_update: bool,
    pub current_page: usize,
    pub results_per_page: usize,
}

#[derive(Default)]
pub struct SearchFields {
    pub name: bool,
    pub set: bool,
    pub condition: bool,
    pub language: bool,
    pub location: bool,
    pub rarity: bool,
    pub price: bool,
    pub comment: bool,
    pub name_de: bool,
    pub name_es: bool,
    pub name_fr: bool,
    pub name_it: bool,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            csv_path: String::new(),
            search_term: String::new(),
            last_search_term: String::new(),
            cards: Vec::new(),
            filtered_cards: Vec::new(),
            search_case_sensitive: false,
            search_in_all_languages: true,
            last_search_time: std::time::Instant::now(),
            search_needs_update: false,
            current_page: 0,
            results_per_page: 100,
            selected_fields: SearchFields {
                name: true,
                set: true,
                condition: false,
                language: false,
                location: false,
                rarity: false,
                price: false,
                comment: false,
                name_de: true,
                name_es: true,
                name_fr: true,
                name_it: true,
            },
        }
    }
}
