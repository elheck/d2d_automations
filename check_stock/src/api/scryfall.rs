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

/// Fetch card image bytes
pub fn fetch_image(url: &str) -> ApiResult<Vec<u8>> {
    Ok(mtg_common::scryfall::blocking::fetch_image(url)?)
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
pub(crate) async fn fetch_card_from_async(
    base_url: &str,
    set_code: &str,
    collector_number: &str,
) -> ApiResult<ScryfallCard> {
    Ok(mtg_common::scryfall::fetch_card_from(base_url, set_code, collector_number).await?)
}

/// Fetch card image bytes (async)
pub async fn fetch_image_async(url: &str) -> ApiResult<Vec<u8>> {
    Ok(mtg_common::scryfall::fetch_image(url).await?)
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
