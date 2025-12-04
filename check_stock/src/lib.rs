pub mod card_matching;
pub mod formatters;
pub mod io;
pub mod models;
pub mod stock_analysis;
pub mod ui;

// Re-export commonly used items
pub use card_matching::{find_matching_cards, MatchedCard};
pub use formatters::{format_picking_list, format_regular_output};
pub use io::{read_csv, read_wantslist};
pub use models::{Card, Language, WantsEntry};
pub use stock_analysis::{format_stock_analysis_with_sort, SortOrder, StockAnalysis, StockStats};
