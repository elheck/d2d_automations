//! API clients for external services (Scryfall, Cardmarket, inventory_sync)

pub mod cardmarket;
pub mod inventory_sync;
pub mod scryfall;

#[cfg(test)]
mod cardmarket_tests;
#[cfg(test)]
mod scryfall_tests;

// Re-exports for public API convenience
#[allow(unused_imports)]
pub use cardmarket::PriceGuide;
#[allow(unused_imports)]
pub use scryfall::{fetch_card, fetch_card_async, fetch_image, fetch_image_async, ScryfallCard};
