//! Cardmarket API client for fetching price guides and product catalogs

mod price_guide;
mod product_catalog;

pub use price_guide::{PriceGuide, PriceGuideEntry};
pub use product_catalog::{ProductCatalog, ProductEntry};
