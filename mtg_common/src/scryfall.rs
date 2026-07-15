use crate::error::{MtgError, MtgResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Base URL of the Scryfall API.
pub const SCRYFALL_API: &str = "https://api.scryfall.com";

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Scryfall image URIs (superset of all fields used across projects).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ImageUris {
    pub small: Option<String>,
    pub normal: Option<String>,
    pub large: Option<String>,
    pub png: Option<String>,
    pub art_crop: Option<String>,
    pub border_crop: Option<String>,
}

/// A single face of a double-faced card.
#[derive(Debug, Deserialize, Serialize, Clone)]
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

/// Scryfall price data.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ScryfallPrices {
    pub eur: Option<String>,
    pub eur_foil: Option<String>,
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
}

/// Purchase links from Scryfall.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PurchaseUris {
    pub cardmarket: Option<String>,
    pub tcgplayer: Option<String>,
}

/// Get the primary image URL (normal size) from image_uris/card_faces.
///
/// Shared logic: tries direct image_uris first, then falls back to
/// the front face of double-faced cards.
pub fn image_url<'a>(
    image_uris: Option<&'a ImageUris>,
    card_faces: Option<&'a [CardFace]>,
) -> Option<&'a str> {
    if let Some(uris) = image_uris {
        return uris.normal.as_deref();
    }
    if let Some(faces) = card_faces {
        if let Some(face) = faces.first() {
            if let Some(ref uris) = face.image_uris {
                return uris.normal.as_deref();
            }
        }
    }
    None
}

/// Scryfall card response (superset of the fields used across projects).
///
/// The identity fields (id, name, set, collector number, rarity) are always
/// present in Scryfall card objects; everything else is optional.
#[derive(Debug, Deserialize, Serialize, Clone)]
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
    #[serde(default)]
    pub purchase_uris: Option<PurchaseUris>,
}

impl ScryfallCard {
    /// Get the primary image URL (normal size)
    pub fn image_url(&self) -> Option<&str> {
        image_url(self.image_uris.as_ref(), self.card_faces.as_deref())
    }
}

/// Scryfall API error response payload.
#[derive(Debug, Deserialize)]
pub struct ScryfallError {
    pub code: String,
    pub details: String,
}

/// URL for fetching a card by set code and collector number.
fn card_url(base_url: &str, set_code: &str, collector_number: &str) -> String {
    format!(
        "{}/cards/{}/{}",
        base_url,
        set_code.to_lowercase(),
        collector_number
    )
}

/// Turn a non-success response body into an error: Scryfall's structured
/// code/details payload when parseable, the bare HTTP status otherwise.
fn error_from_body(status: reqwest::StatusCode, body: &[u8]) -> MtgError {
    match serde_json::from_slice::<ScryfallError>(body) {
        Ok(err) => MtgError::Api {
            code: err.code,
            details: err.details,
        },
        Err(_) => MtgError::HttpStatus(status),
    }
}

/// Fetch a card from Scryfall by set code and collector number.
pub async fn fetch_card(set_code: &str, collector_number: &str) -> MtgResult<ScryfallCard> {
    fetch_card_from(SCRYFALL_API, set_code, collector_number).await
}

/// Fetches a card from the given base URL (for testing with mock servers).
pub async fn fetch_card_from(
    base_url: &str,
    set_code: &str,
    collector_number: &str,
) -> MtgResult<ScryfallCard> {
    let url = card_url(base_url, set_code, collector_number);
    log::info!("Fetching card from Scryfall: {}", url);
    fetch_card_at_url(&url).await
}

/// Fetch a card from Scryfall by Cardmarket product ID.
/// This returns the exact printing matching the Cardmarket listing.
pub async fn fetch_card_by_cardmarket_id(id: u64) -> MtgResult<ScryfallCard> {
    fetch_card_by_cardmarket_id_from(SCRYFALL_API, id).await
}

/// Fetches a card by Cardmarket ID from the given base URL (for testing).
pub async fn fetch_card_by_cardmarket_id_from(base_url: &str, id: u64) -> MtgResult<ScryfallCard> {
    let url = format!("{}/cards/cardmarket/{}", base_url, id);
    log::debug!("Fetching card from Scryfall by cardmarket ID: {}", id);
    fetch_card_at_url(&url).await
}

/// Fetch a card from Scryfall by name (fuzzy search).
/// Note: This returns an arbitrary printing. Prefer `fetch_card_by_cardmarket_id` when possible.
pub async fn fetch_card_by_name(name: &str) -> MtgResult<ScryfallCard> {
    fetch_card_by_name_from(SCRYFALL_API, name).await
}

/// Fetches a card by fuzzy name from the given base URL (for testing).
pub async fn fetch_card_by_name_from(base_url: &str, name: &str) -> MtgResult<ScryfallCard> {
    let url = format!(
        "{}/cards/named?fuzzy={}",
        base_url,
        urlencoding::encode(name)
    );
    log::debug!("Fetching card from Scryfall: {}", name);
    fetch_card_at_url(&url).await
}

async fn fetch_card_at_url(url: &str) -> MtgResult<ScryfallCard> {
    let response = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()?
        .get(url)
        .header("User-Agent", crate::USER_AGENT)
        .send()
        .await?;

    let status = response.status();
    if status.is_success() {
        Ok(response.json::<ScryfallCard>().await?)
    } else {
        let body = response.bytes().await?;
        Err(error_from_body(status, &body))
    }
}

/// Fetch card image bytes from a URL.
pub async fn fetch_image(url: &str) -> MtgResult<Vec<u8>> {
    log::debug!("Fetching image: {}", url);

    let response = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()?
        .get(url)
        .header("User-Agent", crate::USER_AGENT)
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.bytes().await?.to_vec())
    } else {
        Err(MtgError::HttpStatus(response.status()))
    }
}

/// Blocking variants of the fetch functions, for GUI apps without an async runtime.
#[cfg(feature = "blocking")]
pub mod blocking {
    use super::*;

    /// Fetch a card from Scryfall by set code and collector number.
    pub fn fetch_card(set_code: &str, collector_number: &str) -> MtgResult<ScryfallCard> {
        fetch_card_from(SCRYFALL_API, set_code, collector_number)
    }

    /// Fetches a card from the given base URL (for testing with mock servers).
    pub fn fetch_card_from(
        base_url: &str,
        set_code: &str,
        collector_number: &str,
    ) -> MtgResult<ScryfallCard> {
        let url = card_url(base_url, set_code, collector_number);
        log::info!("Fetching card from Scryfall: {}", url);

        let response = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()?
            .get(&url)
            .header("User-Agent", crate::USER_AGENT)
            .send()?;

        let status = response.status();
        if status.is_success() {
            Ok(response.json::<ScryfallCard>()?)
        } else {
            let body = response.bytes()?;
            Err(error_from_body(status, &body))
        }
    }

    /// Fetch card image bytes from a URL.
    pub fn fetch_image(url: &str) -> MtgResult<Vec<u8>> {
        log::debug!("Fetching image: {}", url);

        let response = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()?
            .get(url)
            .header("User-Agent", crate::USER_AGENT)
            .send()?;

        if response.status().is_success() {
            Ok(response.bytes()?.to_vec())
        } else {
            Err(MtgError::HttpStatus(response.status()))
        }
    }
}

#[cfg(test)]
#[path = "scryfall_tests.rs"]
mod tests;
