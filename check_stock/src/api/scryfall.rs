use crate::error::{ApiError, ApiResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

pub use mtg_common::scryfall::{CardFace, ImageUris, ScryfallPrices};

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

impl ScryfallCard {
    /// Get the primary image URL (normal size)
    pub fn image_url(&self) -> Option<&str> {
        mtg_common::scryfall::image_url(self.image_uris.as_ref(), self.card_faces.as_deref())
    }
}

/// Scryfall API error response
#[derive(Debug, Deserialize)]
pub struct ScryfallError {
    pub code: String,
    pub details: String,
}

/// Fetch a card from Scryfall by set code and collector number
pub fn fetch_card(set_code: &str, collector_number: &str) -> ApiResult<ScryfallCard> {
    fetch_card_from("https://api.scryfall.com", set_code, collector_number)
}

/// Fetches a card from the given base URL (for testing with mock servers).
pub(crate) fn fetch_card_from(
    base_url: &str,
    set_code: &str,
    collector_number: &str,
) -> ApiResult<ScryfallCard> {
    let url = format!(
        "{}/cards/{}/{}",
        base_url,
        set_code.to_lowercase(),
        collector_number
    );

    log::info!("Fetching card from Scryfall: {}", url);

    let response = reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()?
        .get(&url)
        .header("User-Agent", mtg_common::USER_AGENT)
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

    let response = reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()?
        .get(url)
        .header("User-Agent", mtg_common::USER_AGENT)
        .send()?;

    if response.status().is_success() {
        Ok(response.bytes()?.to_vec())
    } else {
        Err(ApiError::HttpStatus(response.status()))
    }
}

/// Fetch a card from Scryfall by set code and collector number (async)
pub async fn fetch_card_async(set_code: &str, collector_number: &str) -> ApiResult<ScryfallCard> {
    fetch_card_from_async("https://api.scryfall.com", set_code, collector_number).await
}

/// Fetches a card from the given base URL (async, for testing with mock servers).
pub(crate) async fn fetch_card_from_async(
    base_url: &str,
    set_code: &str,
    collector_number: &str,
) -> ApiResult<ScryfallCard> {
    let url = format!(
        "{}/cards/{}/{}",
        base_url,
        set_code.to_lowercase(),
        collector_number
    );

    log::info!("Fetching card from Scryfall: {}", url);

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
        let error: ScryfallError = response.json().await?;
        Err(ApiError::ApiResponse {
            code: error.code,
            details: error.details,
        })
    }
}

/// Fetch card image bytes (async)
pub async fn fetch_image_async(url: &str) -> ApiResult<Vec<u8>> {
    log::debug!("Fetching image: {}", url);

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
        Err(ApiError::HttpStatus(response.status()))
    }
}
