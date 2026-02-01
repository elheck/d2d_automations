//! Inventory Sync - MTG Stock & Pricing Database
//!
//! This application syncs MTG card inventory from CSV exports to a SQLite database
//! and collects pricing data on a regular schedule.

pub mod cardmarket;
pub mod error;

pub use cardmarket::{PriceGuide, PriceGuideEntry};
pub use error::{Error, Result};
