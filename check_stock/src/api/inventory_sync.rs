//! Inventory-sync REST client — shared implementation lives in `mtg_common`.
//!
//! Client methods return `MtgError`, which converts into this crate's
//! `ApiError` via `From` at call sites.

pub use mtg_common::inventory_sync::{
    InventorySyncClient, LatestPrice, PriceData, PriceField, PriceFields, PriceHistoryPoint,
    PriceSnapshot, ProductSearchResult,
};
