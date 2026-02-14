//! Inventory Sync - MTG Stock & Pricing Database
//!
//! This application syncs MTG card inventory from CSV exports to a SQLite database
//! and collects pricing data on a regular schedule.

pub mod cardmarket;
pub mod database;
pub mod error;
pub mod image_cache;
pub mod scryfall;
pub mod web;

pub use cardmarket::{PriceGuide, PriceGuideEntry, ProductCatalog, ProductEntry};
pub use database::{
    get_price_history, get_product_by_id, has_price_data_for_today, init_schema,
    insert_price_history, search_products_by_name, upsert_products, InsertResult,
    PriceHistoryPoint, ProductSearchResult,
};
pub use error::{Error, Result};
