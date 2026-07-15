//! Scryfall API client — thin wrappers over the shared client in `mtg_common`
//! that convert errors into this crate's `InventoryError`.

use crate::error::InventoryError;
use mtg_common::MtgError;
use serde::{Deserialize, Serialize};

pub use mtg_common::scryfall::{CardFace, ImageUris, PurchaseUris, ScryfallCard};

/// Metadata about a card from Scryfall (serializable for caching)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CardInfo {
    pub set_name: Option<String>,
    pub type_line: Option<String>,
    pub mana_cost: Option<String>,
    pub rarity: Option<String>,
    pub oracle_text: Option<String>,
    pub purchase_uris: Option<PurchaseUris>,
}

impl From<&ScryfallCard> for CardInfo {
    /// Extract cacheable card info from the full Scryfall response
    fn from(card: &ScryfallCard) -> Self {
        CardInfo {
            set_name: Some(card.set_name.clone()),
            type_line: card.type_line.clone(),
            mana_cost: card.mana_cost.clone(),
            rarity: Some(card.rarity.clone()),
            oracle_text: card.oracle_text.clone(),
            purchase_uris: card.purchase_uris.clone(),
        }
    }
}

/// Map a fetch error to ScryfallNotFound for API-level failures (the card
/// doesn't exist), passing through network/parse errors unchanged.
fn not_found_on_api_error(err: MtgError, what: String) -> InventoryError {
    match err {
        MtgError::Api { .. } | MtgError::HttpStatus(_) => InventoryError::ScryfallNotFound(what),
        other => other.into(),
    }
}

/// Fetch a card from Scryfall by Cardmarket product ID.
/// This returns the exact printing matching the Cardmarket listing.
pub async fn fetch_card_by_cardmarket_id(id: u64) -> Result<ScryfallCard, InventoryError> {
    mtg_common::scryfall::fetch_card_by_cardmarket_id(id)
        .await
        .map_err(|e| not_found_on_api_error(e, format!("cardmarket_id:{}", id)))
}

/// Fetch a card from Scryfall by name (fuzzy search).
/// Note: This returns an arbitrary printing. Prefer `fetch_card_by_cardmarket_id` when possible.
pub async fn fetch_card_by_name(name: &str) -> Result<ScryfallCard, InventoryError> {
    mtg_common::scryfall::fetch_card_by_name(name)
        .await
        .map_err(|e| not_found_on_api_error(e, name.to_string()))
}

/// Fetch image bytes from a URL
pub async fn fetch_image(url: &str) -> Result<Vec<u8>, InventoryError> {
    mtg_common::scryfall::fetch_image(url)
        .await
        .map_err(|e| match e {
            MtgError::HttpStatus(_) => InventoryError::ImageFetchFailed(url.to_string()),
            other => other.into(),
        })
}

#[cfg(test)]
#[path = "scryfall_tests.rs"]
mod tests;
