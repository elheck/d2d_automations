pub mod cardmarket;
pub mod error;
pub mod scryfall;

pub use cardmarket::{PriceGuideEntry, PriceGuideFile};
pub use error::{MtgError, MtgResult};
pub use scryfall::{image_url, CardFace, ImageUris, PurchaseUris, ScryfallPrices};

/// Shared User-Agent for all HTTP requests to external APIs.
pub const USER_AGENT: &str = "D2D-Automations/1.0";

/// Cardmarket price guide URL (MTG = category 1).
pub const PRICE_GUIDE_URL: &str =
    "https://downloads.s3.cardmarket.com/productCatalog/priceGuide/price_guide_1.json";
