//! Cardmarket API client for fetching price guides and product catalogs

mod price_guide;
mod product_catalog;

pub use price_guide::{PriceGuide, PriceGuideEntry};
pub use product_catalog::{ProductCatalog, ProductEntry};

#[cfg(test)]
pub use price_guide::make_test_price_entry;
#[cfg(test)]
pub use product_catalog::make_test_product;
