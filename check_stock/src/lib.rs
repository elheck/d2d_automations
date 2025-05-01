pub mod models;
pub mod card_matching;
pub mod formatters;
pub mod io;
pub mod ui;
pub mod stock_analysis;

// Re-export commonly used items
pub use models::{Card, WantsEntry};
pub use card_matching::{find_matching_cards, MatchedCard};
pub use formatters::{format_regular_output, format_picking_list};
pub use io::{read_csv, read_wantslist};
pub use stock_analysis::{StockAnalysis, StockStats, format_stock_analysis};