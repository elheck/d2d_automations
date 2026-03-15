//! Scryfall API client for fetching card images
//!
//! Uses async reqwest for non-blocking HTTP requests.

use crate::error::InventoryError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

pub use mtg_common::scryfall::{CardFace, ImageUris, PurchaseUris};

/// Scryfall card response
#[derive(Debug, Deserialize)]
pub struct ScryfallCard {
    pub name: String,
    #[serde(default)]
    pub image_uris: Option<ImageUris>,
    /// For double-faced cards, images are in card_faces
    #[serde(default)]
    pub card_faces: Option<Vec<CardFace>>,
    #[serde(default)]
    pub set_name: Option<String>,
    #[serde(default)]
    pub type_line: Option<String>,
    #[serde(default)]
    pub mana_cost: Option<String>,
    #[serde(default)]
    pub rarity: Option<String>,
    #[serde(default)]
    pub oracle_text: Option<String>,
    #[serde(default)]
    pub purchase_uris: Option<PurchaseUris>,
}

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

impl ScryfallCard {
    /// Extract cacheable card info from the full Scryfall response
    pub fn card_info(&self) -> CardInfo {
        CardInfo {
            set_name: self.set_name.clone(),
            type_line: self.type_line.clone(),
            mana_cost: self.mana_cost.clone(),
            rarity: self.rarity.clone(),
            oracle_text: self.oracle_text.clone(),
            purchase_uris: self.purchase_uris.clone(),
        }
    }

    /// Get the primary image URL (normal size)
    pub fn image_url(&self) -> Option<&str> {
        mtg_common::scryfall::image_url(self.image_uris.as_ref(), self.card_faces.as_deref())
    }
}

/// Fetch a card from Scryfall by Cardmarket product ID.
/// This returns the exact printing matching the Cardmarket listing.
pub async fn fetch_card_by_cardmarket_id(id: u64) -> Result<ScryfallCard, InventoryError> {
    let url = format!("https://api.scryfall.com/cards/cardmarket/{}", id);

    log::debug!("Fetching card from Scryfall by cardmarket ID: {}", id);

    let response = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()?
        .get(&url)
        .header("User-Agent", mtg_common::USER_AGENT)
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json::<ScryfallCard>().await?)
    } else {
        Err(InventoryError::ScryfallNotFound(format!(
            "cardmarket_id:{}",
            id
        )))
    }
}

/// Fetch a card from Scryfall by name (fuzzy search).
/// Note: This returns an arbitrary printing. Prefer `fetch_card_by_cardmarket_id` when possible.
pub async fn fetch_card_by_name(name: &str) -> Result<ScryfallCard, InventoryError> {
    let url = format!(
        "https://api.scryfall.com/cards/named?fuzzy={}",
        urlencoding::encode(name)
    );

    log::debug!("Fetching card from Scryfall: {}", name);

    let response = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()?
        .get(&url)
        .header("User-Agent", mtg_common::USER_AGENT)
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json::<ScryfallCard>().await?)
    } else {
        Err(InventoryError::ScryfallNotFound(name.to_string()))
    }
}

/// Fetch image bytes from a URL
pub async fn fetch_image(url: &str) -> Result<Vec<u8>, InventoryError> {
    log::debug!("Fetching image from URL: {}", url);

    let response = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()?
        .get(url)
        .header("User-Agent", mtg_common::USER_AGENT)
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.bytes().await?.to_vec())
    } else {
        Err(InventoryError::ImageFetchFailed(url.to_string()))
    }
}

#[cfg(test)]
#[path = "scryfall_tests.rs"]
mod tests;
