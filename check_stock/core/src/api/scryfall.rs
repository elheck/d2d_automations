//! Scryfall API client — thin wrappers over the shared client in `mtg_common`
//! that convert errors into this crate's `ApiError`.

use crate::error::ApiResult;

pub use mtg_common::scryfall::{CardFace, ImageUris, ScryfallCard, ScryfallError, ScryfallPrices};

/// Fetch a card from Scryfall by set code and collector number
pub fn fetch_card(set_code: &str, collector_number: &str) -> ApiResult<ScryfallCard> {
    fetch_card_from(
        mtg_common::scryfall::SCRYFALL_API,
        set_code,
        collector_number,
    )
}

/// Fetches a card from the given base URL (for testing with mock servers).
pub(crate) fn fetch_card_from(
    base_url: &str,
    set_code: &str,
    collector_number: &str,
) -> ApiResult<ScryfallCard> {
    Ok(mtg_common::scryfall::blocking::fetch_card_from(
        base_url,
        set_code,
        collector_number,
    )?)
}

/// Fetch a card from Scryfall by set code and collector number (async)
pub async fn fetch_card_async(set_code: &str, collector_number: &str) -> ApiResult<ScryfallCard> {
    fetch_card_from_async(
        mtg_common::scryfall::SCRYFALL_API,
        set_code,
        collector_number,
    )
    .await
}

/// Fetches a card from the given base URL (async, for testing with mock servers).
pub async fn fetch_card_from_async(
    base_url: &str,
    set_code: &str,
    collector_number: &str,
) -> ApiResult<ScryfallCard> {
    Ok(mtg_common::scryfall::fetch_card_from(base_url, set_code, collector_number).await?)
}

/// Fetch image bytes from any URL. Crate-private: callers use
/// [`fetch_scryfall_image_async`], which only allows Scryfall's image host.
pub(crate) async fn fetch_image_async(url: &str) -> ApiResult<Vec<u8>> {
    Ok(mtg_common::scryfall::fetch_image(url).await?)
}

/// Host Scryfall serves card images from.
pub const SCRYFALL_IMAGE_HOST: &str = "cards.scryfall.io";

/// Whether `url` is a Scryfall card image (`https://cards.scryfall.io/…`).
///
/// Image URLs come from Scryfall's API responses; fetching whatever such a
/// response names would let a tampered response point the app at internal
/// addresses (server-side request forgery). Only this exact origin passes:
/// https, the standard port, no user info.
pub fn is_scryfall_image_url(url: &str) -> bool {
    match reqwest::Url::parse(url) {
        Ok(u) => {
            u.scheme() == "https"
                && u.host_str() == Some(SCRYFALL_IMAGE_HOST)
                && u.port().is_none()
                && u.username().is_empty()
                && u.password().is_none()
        }
        Err(_) => false,
    }
}

/// Fetch a card image, but only from Scryfall's image host
/// ([`is_scryfall_image_url`]); anything else is refused without a request.
pub async fn fetch_scryfall_image_async(url: &str) -> ApiResult<Vec<u8>> {
    if !is_scryfall_image_url(url) {
        return Err(crate::error::ApiError::ApiResponse {
            code: "untrusted_image_url".to_string(),
            details: format!("not a Scryfall image URL: {url}"),
        });
    }
    fetch_image_async(url).await
}

/// Look up to 75 printings (set + collector number) in one request (async).
pub async fn fetch_collection_from_async(
    base_url: &str,
    identifiers: &[mtg_common::scryfall::CollectionIdentifier],
) -> ApiResult<Vec<ScryfallCard>> {
    Ok(mtg_common::scryfall::fetch_collection_from(base_url, identifiers).await?)
}

/// Fetch the exact printing behind a Cardmarket product id (async).
pub async fn fetch_card_by_cardmarket_id_from_async(
    base_url: &str,
    id: u64,
) -> ApiResult<ScryfallCard> {
    Ok(mtg_common::scryfall::fetch_card_by_cardmarket_id_from(base_url, id).await?)
}
