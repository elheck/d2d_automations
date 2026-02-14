//! Scryfall API client for fetching card images
//!
//! Uses async reqwest for non-blocking HTTP requests.

use crate::error::InventoryError;
use serde::Deserialize;

/// Scryfall card response
#[derive(Debug, Deserialize)]
pub struct ScryfallCard {
    pub name: String,
    #[serde(default)]
    pub image_uris: Option<ImageUris>,
    /// For double-faced cards, images are in card_faces
    #[serde(default)]
    pub card_faces: Option<Vec<CardFace>>,
}

#[derive(Debug, Deserialize)]
pub struct ImageUris {
    pub small: Option<String>,
    pub normal: Option<String>,
    pub large: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CardFace {
    pub name: String,
    #[serde(default)]
    pub image_uris: Option<ImageUris>,
}

impl ScryfallCard {
    /// Get the primary image URL (normal size)
    pub fn image_url(&self) -> Option<&str> {
        // Try direct image_uris first
        if let Some(ref uris) = self.image_uris {
            return uris.normal.as_deref();
        }
        // For double-faced cards, get front face image
        if let Some(ref faces) = self.card_faces {
            if let Some(face) = faces.first() {
                if let Some(ref uris) = face.image_uris {
                    return uris.normal.as_deref();
                }
            }
        }
        None
    }
}

/// Fetch a card from Scryfall by name (fuzzy search)
pub async fn fetch_card_by_name(name: &str) -> Result<ScryfallCard, InventoryError> {
    let url = format!(
        "https://api.scryfall.com/cards/named?fuzzy={}",
        urlencoding::encode(name)
    );

    log::debug!("Fetching card from Scryfall: {}", name);

    let response = reqwest::Client::new()
        .get(&url)
        .header("User-Agent", "D2D-Automations-InventorySync/1.0")
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

    let response = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "D2D-Automations-InventorySync/1.0")
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
