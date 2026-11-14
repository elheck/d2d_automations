//! Singles listing (permission `listing.use`): look up a printing by set code
//! and collector number, with its image and prices.
//!
//! - `GET /api/singles/lookup?input=hou120&default_set=hou`: the card, its
//!   Scryfall prices and the latest Cardmarket prices from inventory_sync
//! - `GET /api/singles/image/{set}/{number}`: the card image (JPEG or PNG)
//!
//! Nothing is written: no audit entries, no database access.

use super::{Access, Api, ApiQuery};
use crate::app::AppState;
use crate::cards::LookupError;
use crate::http::ApiError;
use crate::permissions::Permission;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use check_stock_core::api::cardmarket::product_url;
use check_stock_core::api::inventory_sync::LatestPrice;
use check_stock_core::api::scryfall::ScryfallCard;
use check_stock_core::cache::ImageCache;
use check_stock_core::card_input::resolve_card_input;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use ts_rs::TS;

/// Longest accepted card input (`hou120`, with room for long set codes).
pub const MAX_INPUT_LEN: usize = 24;
/// Longest accepted default set code.
pub const MAX_SET_LEN: usize = 10;
/// How long a price lookup at inventory_sync may take before the card is
/// shown without Cardmarket prices.
pub const PRICE_TIMEOUT: Duration = Duration::from_secs(5);
/// Browsers may keep card images this long (they are public card art).
pub const IMAGE_MAX_AGE_SECS: u32 = 7 * 24 * 60 * 60;

pub(super) fn routes(api: Api) -> Api {
    api.route(
        "/api/singles/lookup",
        Access::Permission(Permission::ListingUse),
        get(lookup),
    )
    .route(
        "/api/singles/image/{set}/{number}",
        Access::Permission(Permission::ListingUse),
        get(image),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LookupQuery {
    input: String,
    #[serde(default)]
    default_set: String,
}

/// One printing, as the singles listing shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SingleCard {
    /// Lower-case set code as looked up, e.g. `hou`.
    pub set_code: String,
    /// Collector number as looked up, e.g. `120`.
    pub collector_number: String,
    pub name: String,
    pub set_name: String,
    pub rarity: String,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    /// Scryfall's euro prices (they come from Cardmarket, updated daily).
    pub scryfall_eur: Option<f64>,
    pub scryfall_eur_foil: Option<f64>,
    pub cardmarket_id: Option<u64>,
    /// The printing's Cardmarket product page.
    pub cardmarket_url: Option<String>,
    /// Path of the image route for this card; `None` when Scryfall has none.
    pub image_path: Option<String>,
    pub prices: PriceLookup,
}

/// Cardmarket prices of a printing, or why there are none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "status", rename_all = "snake_case")]
#[ts(export)]
pub enum PriceLookup {
    /// The latest daily row inventory_sync has.
    Found {
        /// Date of the price row (`YYYY-MM-DD`).
        price_date: String,
        // Boxed: the other variants carry nothing (same JSON either way).
        regular: Box<PriceSet>,
        foil: Box<PriceSet>,
    },
    /// Scryfall knows no Cardmarket product for this printing.
    NoCardmarketId,
    /// inventory_sync has no prices for the product.
    NotTracked,
    /// The server was started without `--inventory-sync-url`.
    NotConfigured,
    /// inventory_sync did not answer (in time).
    Unavailable,
}

/// The price guide values of one finish (euros).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PriceSet {
    pub trend: Option<f64>,
    pub low: Option<f64>,
    pub avg: Option<f64>,
    pub avg1: Option<f64>,
    pub avg7: Option<f64>,
    pub avg30: Option<f64>,
}

impl PriceSet {
    fn regular(p: &LatestPrice) -> Self {
        Self {
            trend: price(p.trend),
            low: price(p.low),
            avg: price(p.avg),
            avg1: price(p.avg1),
            avg7: price(p.avg7),
            avg30: price(p.avg30),
        }
    }

    fn foil(p: &LatestPrice) -> Self {
        Self {
            trend: price(p.trend_foil),
            low: price(p.low_foil),
            avg: price(p.avg_foil),
            avg1: price(p.avg1_foil),
            avg7: price(p.avg7_foil),
            avg30: price(p.avg30_foil),
        }
    }
}

/// Prices `<= 0` (or not finite) mean "no price".
fn price(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite() && *v > 0.0)
}

/// Scryfall's price strings (`"0.25"`).
fn scryfall_price(value: Option<&str>) -> Option<f64> {
    price(value.and_then(|v| v.trim().parse().ok()))
}

impl SingleCard {
    fn from_card(
        set_code: &str,
        collector_number: &str,
        card: &ScryfallCard,
        prices: PriceLookup,
    ) -> Self {
        let front = card.card_faces.as_deref().and_then(|faces| faces.first());
        let mana_cost = card
            .mana_cost
            .clone()
            .filter(|m| !m.is_empty())
            .or_else(|| {
                let faces = card.card_faces.as_deref()?;
                let costs: Vec<&str> = faces
                    .iter()
                    .filter_map(|f| f.mana_cost.as_deref())
                    .filter(|m| !m.is_empty())
                    .collect();
                (!costs.is_empty()).then(|| costs.join(" // "))
            });
        let type_line = card
            .type_line
            .clone()
            .or_else(|| front.and_then(|f| f.type_line.clone()));
        Self {
            set_code: set_code.to_string(),
            collector_number: collector_number.to_string(),
            name: card.name.clone(),
            set_name: card.set_name.clone(),
            rarity: card.rarity.clone(),
            mana_cost,
            type_line,
            scryfall_eur: scryfall_price(card.prices.eur.as_deref()),
            scryfall_eur_foil: scryfall_price(card.prices.eur_foil.as_deref()),
            cardmarket_id: card.cardmarket_id,
            cardmarket_url: card.cardmarket_id.map(product_url),
            image_path: card
                .image_url()
                .is_some()
                .then(|| image_path(set_code, collector_number)),
            prices,
        }
    }
}

/// Path of the image route (both parts are [safe keys](ImageCache::is_safe_key),
/// so they need no escaping).
fn image_path(set_code: &str, collector_number: &str) -> String {
    format!("/api/singles/image/{set_code}/{collector_number}")
}

fn invalid_input() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_card_input",
        "Enter a set code followed by a three-digit collector number, e.g. hou120 (or only the number with a default set)",
    )
}

fn lookup_error(e: LookupError) -> ApiError {
    match e {
        LookupError::InvalidKey => invalid_input(),
        LookupError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "card_not_found",
            "Scryfall knows no card with this set code and collector number",
        ),
        LookupError::NoImage => ApiError::new(
            StatusCode::NOT_FOUND,
            "no_image",
            "Scryfall has no image of this card",
        ),
        LookupError::Unavailable => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "scryfall_unavailable",
            "Scryfall could not be reached; try again in a moment",
        ),
    }
}

async fn lookup(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<LookupQuery>,
) -> Result<Json<SingleCard>, ApiError> {
    if query.input.len() > MAX_INPUT_LEN || query.default_set.len() > MAX_SET_LEN {
        return Err(invalid_input());
    }
    let card_ref =
        resolve_card_input(&query.input, &query.default_set).ok_or_else(invalid_input)?;
    if !ImageCache::is_safe_key(&card_ref.set_code, &card_ref.collector_number) {
        return Err(invalid_input());
    }
    let card = state
        .cards
        .card(&card_ref.set_code, &card_ref.collector_number)
        .await
        .map_err(lookup_error)?;
    let prices = cardmarket_prices(&state, card.cardmarket_id).await;
    Ok(Json(SingleCard::from_card(
        &card_ref.set_code,
        &card_ref.collector_number,
        &card,
        prices,
    )))
}

/// The latest Cardmarket prices of `cardmarket_id` from inventory_sync.
async fn cardmarket_prices(state: &AppState, cardmarket_id: Option<u64>) -> PriceLookup {
    let Some(id) = cardmarket_id else {
        return PriceLookup::NoCardmarketId;
    };
    let Some(client) = state.inventory_sync.as_ref() else {
        return PriceLookup::NotConfigured;
    };
    match tokio::time::timeout(PRICE_TIMEOUT, client.latest_prices(&[id])).await {
        Ok(Ok(rows)) => match rows.iter().find(|row| row.id_product == id) {
            Some(row) => PriceLookup::Found {
                price_date: row.price_date.clone(),
                regular: Box::new(PriceSet::regular(row)),
                foil: Box::new(PriceSet::foil(row)),
            },
            None => PriceLookup::NotTracked,
        },
        Ok(Err(e)) => {
            log::warn!("inventory_sync price lookup for {id} failed: {e}");
            PriceLookup::Unavailable
        }
        Err(_) => {
            log::warn!("inventory_sync price lookup for {id} timed out");
            PriceLookup::Unavailable
        }
    }
}

async fn image(
    State(state): State<AppState>,
    Path((set_code, collector_number)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    // Same normalisation as the lookup, so both use one cache entry.
    let set_code = set_code.to_ascii_lowercase();
    if !ImageCache::is_safe_key(&set_code, &collector_number) {
        return Err(invalid_input());
    }
    let (bytes, kind) = state
        .cards
        .image(&set_code, &collector_number)
        .await
        .map_err(lookup_error)?;
    let cache_control = HeaderValue::from_str(&format!("private, max-age={IMAGE_MAX_AGE_SECS}"))
        .map_err(|_| ApiError::internal())?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static(kind.content_type()),
            ),
            (header::CACHE_CONTROL, cache_control),
        ],
        bytes,
    )
        .into_response())
}

#[cfg(test)]
#[path = "singles_tests.rs"]
mod tests;
