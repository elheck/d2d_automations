//! Inventory Sync - MTG Stock & Pricing Database
//!
//! This application syncs MTG card inventory from CSV exports to a SQLite database
//! and collects pricing data on a regular schedule.

pub mod cardmarket;
pub mod database;
pub mod error;

pub use cardmarket::{PriceGuide, PriceGuideEntry, ProductCatalog, ProductEntry};
pub use database::{
    has_price_data_for_today, init_schema, insert_price_history, upsert_products, InsertResult,
};
pub use error::{Error, Result};
