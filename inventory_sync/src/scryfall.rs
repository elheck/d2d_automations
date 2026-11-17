//! Scryfall API client — thin wrappers over the shared client in `mtg_common`
//! that convert errors into this crate's `InventoryError`.

use crate::error::InventoryError;
use mtg_common::MtgError;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

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

// ── The server's Scryfall client ───────────────────────────────────────────

/// Only Scryfall's "not found" counts as a miss; any other error (rate
/// limit, outage) is passing and must not hide the card for 30 days.
fn card_error(err: MtgError, id: u64) -> InventoryError {
    let what = format!("cardmarket_id:{id}");
    match err {
        MtgError::Api { code, .. } if code == "not_found" => InventoryError::ScryfallNotFound(what),
        MtgError::HttpStatus(s) if s == reqwest::StatusCode::NOT_FOUND => {
            InventoryError::ScryfallNotFound(what)
        }
        MtgError::Api { code, .. } => {
            InventoryError::ScryfallUnavailable(format!("{what} ({code})"))
        }
        MtgError::HttpStatus(s) => InventoryError::ScryfallUnavailable(format!("{what} ({s})")),
        other => other.into(),
    }
}

/// Where card images come from; nothing else is ever downloaded as an image.
pub const SCRYFALL_IMAGE_PREFIX: &str = "https://cards.scryfall.io/";
/// Pause between two Scryfall requests (Scryfall asks for 50–100 ms).
pub const SCRYFALL_SPACING: Duration = Duration::from_millis(100);
/// Largest image accepted (Scryfall's `normal` JPEGs are about 150 KB).
pub const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
const IMAGE_TIMEOUT: Duration = Duration::from_secs(30);

/// Scryfall as the web server uses it: one request at a time, spaced, card
/// data from the API and images only from Scryfall's image host (no
/// redirects, size-capped, JPEG or PNG only).
pub struct Scryfall {
    api: String,
    image_prefix: String,
    spacing: Duration,
    last: tokio::sync::Mutex<Option<Instant>>,
}

impl Default for Scryfall {
    fn default() -> Self {
        Self::with_endpoints(
            mtg_common::scryfall::SCRYFALL_API,
            SCRYFALL_IMAGE_PREFIX,
            SCRYFALL_SPACING,
        )
    }
}

impl Scryfall {
    /// Other endpoints (tests: a fake Scryfall).
    pub fn with_endpoints(api: &str, image_prefix: &str, spacing: Duration) -> Self {
        Self {
            api: api.trim_end_matches('/').to_string(),
            image_prefix: image_prefix.to_string(),
            spacing,
            last: tokio::sync::Mutex::new(None),
        }
    }

    /// Runs `request` when it is this caller's turn.
    async fn in_turn<T>(&self, request: impl std::future::Future<Output = T>) -> T {
        let mut last = self.last.lock().await;
        if let Some(at) = *last {
            let wait = self.spacing.saturating_sub(at.elapsed());
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
        }
        let result = request.await;
        *last = Some(Instant::now());
        result
    }

    /// The card with this Cardmarket product id.
    pub async fn card(&self, id: u64) -> Result<ScryfallCard, InventoryError> {
        self.in_turn(mtg_common::scryfall::fetch_card_by_cardmarket_id_from(
            &self.api, id,
        ))
        .await
        .map_err(|e| card_error(e, id))
    }

    /// An image from Scryfall's image host.
    pub async fn image(&self, url: &str) -> Result<Vec<u8>, InventoryError> {
        if !url.starts_with(&self.image_prefix) {
            log::warn!("refused an image outside Scryfall's image host: {url}");
            return Err(InventoryError::ImageFetchFailed(
                "not a Scryfall image".into(),
            ));
        }
        self.in_turn(download_image(url)).await
    }
}

async fn download_image(url: &str) -> Result<Vec<u8>, InventoryError> {
    let client = reqwest::Client::builder()
        .timeout(IMAGE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = client
        .get(url)
        .header("User-Agent", mtg_common::USER_AGENT)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(InventoryError::HttpStatus(response.status()));
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_IMAGE_BYTES as u64)
    {
        return Err(InventoryError::ImageFetchFailed("image too large".into()));
    }
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(InventoryError::ImageFetchFailed("image too large".into()));
    }
    if image_type(&bytes).is_none() {
        return Err(InventoryError::ImageFetchFailed("not a JPEG or PNG".into()));
    }
    Ok(bytes.to_vec())
}

/// The media type of an image by its first bytes (JPEG or PNG only).
pub fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else {
        None
    }
}

#[cfg(test)]
#[path = "scryfall_tests.rs"]
mod tests;
