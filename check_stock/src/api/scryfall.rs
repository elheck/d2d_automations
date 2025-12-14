use crate::error::{ApiError, ApiResult};
use serde::{Deserialize, Serialize};

/// Scryfall card response
#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(dead_code)]
pub struct ScryfallCard {
    pub id: String,
    pub name: String,
    pub set: String,
    pub set_name: String,
    pub collector_number: String,
    pub rarity: String,
    #[serde(default)]
    pub prices: ScryfallPrices,
    #[serde(default)]
    pub image_uris: Option<ImageUris>,
    /// For double-faced cards, images are in card_faces
    #[serde(default)]
    pub card_faces: Option<Vec<CardFace>>,
    /// Cardmarket product ID for price matching
    #[serde(default)]
    pub cardmarket_id: Option<u64>,
    #[serde(default)]
    pub mana_cost: Option<String>,
    #[serde(default)]
    pub type_line: Option<String>,
    #[serde(default)]
    pub oracle_text: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[allow(dead_code)]
pub struct ScryfallPrices {
    pub eur: Option<String>,
    pub eur_foil: Option<String>,
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(dead_code)]
pub struct ImageUris {
    pub small: Option<String>,
    pub normal: Option<String>,
    pub large: Option<String>,
    pub png: Option<String>,
    pub art_crop: Option<String>,
    pub border_crop: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(dead_code)]
pub struct CardFace {
    pub name: String,
    #[serde(default)]
    pub image_uris: Option<ImageUris>,
    #[serde(default)]
    pub mana_cost: Option<String>,
    #[serde(default)]
    pub type_line: Option<String>,
    #[serde(default)]
    pub oracle_text: Option<String>,
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

/// Scryfall API error response
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ScryfallError {
    pub status: u16,
    pub code: String,
    pub details: String,
}

/// Fetch a card from Scryfall by set code and collector number
pub fn fetch_card(set_code: &str, collector_number: &str) -> ApiResult<ScryfallCard> {
    let url = format!(
        "https://api.scryfall.com/cards/{}/{}",
        set_code.to_lowercase(),
        collector_number
    );

    log::info!("Fetching card from Scryfall: {}", url);

    let response = reqwest::blocking::Client::new()
        .get(&url)
        .header("User-Agent", "D2D-Automations/1.0")
        .send()?;

    if response.status().is_success() {
        Ok(response.json::<ScryfallCard>()?)
    } else {
        let error: ScryfallError = response.json()?;
        Err(ApiError::ApiResponse {
            code: error.code,
            details: error.details,
        })
    }
}

/// Fetch card image bytes
pub fn fetch_image(url: &str) -> ApiResult<Vec<u8>> {
    log::debug!("Fetching image: {}", url);

    let response = reqwest::blocking::Client::new()
        .get(url)
        .header("User-Agent", "D2D-Automations/1.0")
        .send()?;

    if response.status().is_success() {
        Ok(response.bytes()?.to_vec())
    } else {
        Err(ApiError::HttpStatus(response.status()))
    }
}
