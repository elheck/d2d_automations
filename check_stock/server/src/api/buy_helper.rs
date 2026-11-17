//! Card Buy Helper (permission `buy_helper.use`): values a seller's card list
//! for a purchase offer.
//!
//! - `POST /api/buy-helper/offer`: `{csv, params}` → summary and the most
//!   valuable singles
//! - `POST /api/buy-helper/export`: same body → the offer as a CSV download
//! - `GET` / `PUT /api/buy-helper/settings`: the user's own split rules and
//!   rates, kept on the server ([`crate::user_settings`])
//!
//! The card list is never stored: the browser sends the file's text with
//! every request, the server parses it in memory, answers and forgets it.
//! The upload is the only request body larger than the usual 64 KiB, and
//! only on the offer and export routes; it is capped in size, rows,
//! quantities and prices. Only a changed setting is written (with its audit
//! entry).

use super::{blocking, in_transaction, Access, Api, ApiJson, CurrentUser};
use crate::app::AppState;
use crate::audit::{self, action};
use crate::http::ApiError;
use crate::permissions::Permission;
use crate::user_settings::{self, Setting, SettingValue};
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use check_stock_core::buy_helper::{
    card_qty, classify, compute_summary, export_csv, BuyParams, BuySummary, CardClass,
};
use check_stock_core::io::{parse_csv, CsvReadError};
use check_stock_core::models::Card;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Largest card list the browser may send (the file's text).
pub const MAX_CSV_BYTES: usize = 6 * 1024 * 1024;
/// Request body limit of these routes: the CSV inside JSON (escaping makes
/// it somewhat larger) plus the settings.
pub const BODY_LIMIT: usize = MAX_CSV_BYTES + MAX_CSV_BYTES / 2;
/// Most rows read from one file (counted before empty rows are skipped).
pub const MAX_ROWS: usize = 50_000;
/// Largest quantity and unit price accepted on one row.
pub const MAX_QUANTITY: i64 = 1_000_000;
pub const MAX_PRICE: f64 = 1_000_000.0;
/// Largest money amount accepted in the settings (euros).
pub const MAX_AMOUNT: f64 = 1_000_000.0;
/// Largest bulk batch size.
pub const MAX_BULK_BATCH: u32 = 1_000_000;
/// Singles listed in the answer; the export has all of them.
pub const PREVIEW_LIMIT: usize = 200;

pub(super) fn routes(api: Api) -> Api {
    api.route(
        "/api/buy-helper/offer",
        Access::Permission(Permission::BuyHelperUse),
        post(offer).layer(DefaultBodyLimit::max(BODY_LIMIT)),
    )
    .route(
        "/api/buy-helper/export",
        Access::Permission(Permission::BuyHelperUse),
        post(export).layer(DefaultBodyLimit::max(BODY_LIMIT)),
    )
    .route(
        "/api/buy-helper/settings",
        Access::Permission(Permission::BuyHelperUse),
        get(get_settings).put(put_settings),
    )
}

/// A card list with the settings to value it with.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct BuyOfferRequest {
    /// The seller's card export (inventory CSV format), as text.
    pub csv: String,
    pub params: BuyOfferParams,
}

/// How cards are split into singles and bulk, and what is offered for each.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct BuyOfferParams {
    /// Rarities that make a card a single.
    pub single_common: bool,
    pub single_uncommon: bool,
    pub single_rare: bool,
    pub single_mythic: bool,
    /// Cards worth at least this much (euros) are singles; `null` = no price rule.
    pub min_single_price: Option<f64>,
    /// Share of a single's value offered, 0–100.
    pub single_buy_percent: f64,
    /// Bulk: `bulk_rate` euros per `bulk_batch` cards.
    pub bulk_rate: f64,
    pub bulk_batch: u32,
}

impl From<BuyParams> for BuyOfferParams {
    fn from(p: BuyParams) -> Self {
        Self {
            single_common: p.single_common,
            single_uncommon: p.single_uncommon,
            single_rare: p.single_rare,
            single_mythic: p.single_mythic,
            min_single_price: p.min_single_price,
            single_buy_percent: p.single_buy_percent,
            bulk_rate: p.bulk_rate,
            bulk_batch: p.bulk_batch,
        }
    }
}

impl BuyOfferParams {
    /// Core's parameters, if every value is in range.
    fn validate(&self) -> Result<BuyParams, ApiError> {
        let amount_ok = |v: f64| v.is_finite() && (0.0..=MAX_AMOUNT).contains(&v);
        if !self.single_buy_percent.is_finite() || !(0.0..=100.0).contains(&self.single_buy_percent)
        {
            return Err(invalid_params(
                "The share for singles must be between 0 and 100 %",
            ));
        }
        if !amount_ok(self.bulk_rate) {
            return Err(invalid_params(
                "The bulk rate must be between 0 and 1,000,000 €",
            ));
        }
        if !(1..=MAX_BULK_BATCH).contains(&self.bulk_batch) {
            return Err(invalid_params(
                "The bulk batch must be between 1 and 1,000,000 cards",
            ));
        }
        if self.min_single_price.is_some_and(|p| !amount_ok(p)) {
            return Err(invalid_params(
                "The single price must be between 0 and 1,000,000 €",
            ));
        }
        Ok(BuyParams {
            single_common: self.single_common,
            single_uncommon: self.single_uncommon,
            single_rare: self.single_rare,
            single_mythic: self.single_mythic,
            min_single_price: self.min_single_price,
            single_buy_percent: self.single_buy_percent,
            bulk_rate: self.bulk_rate,
            bulk_batch: self.bulk_batch,
        })
    }
}

/// The offer for a card list.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct BuyOffer {
    /// Card rows read (rows without price or quantity, or with quantity 0,
    /// are not counted).
    pub rows: usize,
    pub summary: BuyOfferSummary,
    /// The most valuable singles first, at most [`PREVIEW_LIMIT`].
    pub singles: Vec<BuyOfferSingle>,
    /// Number of single rows in total.
    pub singles_total: usize,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct BuyOfferSummary {
    pub single_rows: usize,
    pub single_cards: i64,
    pub single_market_value: f64,
    pub single_offer: f64,
    pub bulk_rows: usize,
    pub bulk_cards: i64,
    pub bulk_market_value: f64,
    pub bulk_offer: f64,
    pub total_offer: f64,
}

impl From<BuySummary> for BuyOfferSummary {
    fn from(s: BuySummary) -> Self {
        Self {
            single_rows: s.single_rows,
            single_cards: s.single_cards,
            single_market_value: s.single_market_value,
            single_offer: s.single_offer,
            bulk_rows: s.bulk_rows,
            bulk_cards: s.bulk_cards,
            bulk_market_value: s.bulk_market_value,
            bulk_offer: s.bulk_offer,
            total_offer: s.total_offer,
        }
    }
}

/// One single of the list.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct BuyOfferSingle {
    pub name: String,
    pub set_code: String,
    pub rarity: String,
    pub quantity: i64,
    /// Unit price from the seller's file (euros).
    pub unit_price: f64,
    /// Offered for all copies of this row.
    pub offer: f64,
}

fn invalid_params(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_params", message)
}

fn invalid_csv(message: String) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_csv", message)
}

/// The request body, with an upload-specific message when it is too large.
fn body(request: Result<ApiJson<BuyOfferRequest>, ApiError>) -> Result<BuyOfferRequest, ApiError> {
    match request {
        Ok(ApiJson(request)) => Ok(request),
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => Err(too_large()),
        Err(e) => Err(e),
    }
}

fn too_large() -> ApiError {
    ApiError::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        "too_large",
        format!(
            "The card list is too large (at most {} MB)",
            MAX_CSV_BYTES / (1024 * 1024)
        ),
    )
}

/// Parses and checks the card list (on a blocking thread: up to a few MB of
/// CSV).
async fn read_request(request: BuyOfferRequest) -> Result<(Vec<Card>, BuyParams), ApiError> {
    let params = request.params.validate()?;
    if request.csv.len() > MAX_CSV_BYTES {
        return Err(too_large());
    }
    let cards = blocking(move || parse_csv(request.csv.as_bytes(), MAX_ROWS)).await?;
    let cards =
        cards.map_err(|e| match e {
            CsvReadError::TooManyRows { max } => invalid_csv(format!(
                "The card list has more than {max} rows; split it into several files"
            )),
            CsvReadError::Csv(_) => invalid_csv(format!(
            "This is not a card export{}: it needs the columns of the Cardmarket inventory export \
             (cardmarketId, quantity, name, condition, language, price) and must be UTF-8 text",
            e.line().map(|l| format!(" (line {l} cannot be read)")).unwrap_or_default()
        )),
        })?;
    check_values(&cards)?;
    Ok((cards, params))
}

/// Refuses quantities and prices that are out of range (they would distort
/// or overflow the sums).
fn check_values(cards: &[Card]) -> Result<(), ApiError> {
    let bad = cards.iter().find(|card| {
        let price = card.price_f64();
        card_qty(card) > MAX_QUANTITY || !(0.0..=MAX_PRICE).contains(&price)
    });
    match bad {
        None => Ok(()),
        Some(card) => Err(invalid_csv(format!(
            "\"{}\" has a quantity above 1,000,000 or a price below 0 or above 1,000,000 €",
            short(&card.name)
        ))),
    }
}

/// At most 60 characters of `text` (for messages).
fn short(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(60).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

async fn offer(
    request: Result<ApiJson<BuyOfferRequest>, ApiError>,
) -> Result<Json<BuyOffer>, ApiError> {
    let (cards, params) = read_request(body(request)?).await?;
    Ok(Json(build_offer(&cards, &params)))
}

/// Summary plus the most valuable singles (ties by name, so the list is
/// stable).
fn build_offer(cards: &[Card], params: &BuyParams) -> BuyOffer {
    let mut singles: Vec<&Card> = cards
        .iter()
        .filter(|card| classify(card, params) == CardClass::Single)
        .collect();
    singles.sort_by(|a, b| {
        b.price_f64()
            .total_cmp(&a.price_f64())
            .then_with(|| a.name.cmp(&b.name))
    });
    BuyOffer {
        rows: cards.len(),
        summary: compute_summary(cards, params).into(),
        singles_total: singles.len(),
        singles: singles
            .iter()
            .take(PREVIEW_LIMIT)
            .map(|card| {
                let quantity = card_qty(card);
                let unit_price = card.price_f64();
                BuyOfferSingle {
                    name: card.name.clone(),
                    set_code: card.set_code.clone(),
                    rarity: card.rarity.clone(),
                    quantity,
                    unit_price,
                    offer: unit_price * quantity as f64 * params.single_buy_percent / 100.0,
                }
            })
            .collect(),
    }
}

async fn export(request: Result<ApiJson<BuyOfferRequest>, ApiError>) -> Result<Response, ApiError> {
    let (cards, params) = read_request(body(request)?).await?;
    let csv = blocking(move || export_csv(&cards, &params))
        .await?
        .map_err(|e| {
            log::error!("buy helper export failed: {e}");
            ApiError::internal()
        })?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/csv; charset=utf-8"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"buy_offer.csv\""),
            ),
        ],
        csv,
    )
        .into_response())
}

/// The user's settings, and the defaults (for "reset").
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct BuyHelperSettings {
    pub params: BuyOfferParams,
    pub defaults: BuyOfferParams,
}

fn defaults() -> BuyOfferParams {
    BuyParams::default().into()
}

/// `GET /api/buy-helper/settings`: the stored settings; the defaults when
/// there are none or they no longer fit (e.g. written by an older version).
async fn get_settings(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<BuyHelperSettings>, ApiError> {
    let user_id = user.id;
    let stored = state
        .db
        .read(move |conn| user_settings::get(conn, user_id, Setting::BuyHelper))
        .await?;
    let params = stored
        .and_then(|value| serde_json::from_value::<BuyOfferParams>(value).ok())
        .filter(|params| params.validate().is_ok())
        .unwrap_or_else(defaults);
    Ok(Json(BuyHelperSettings {
        params,
        defaults: defaults(),
    }))
}

/// `PUT /api/buy-helper/settings`: stores the user's settings (only when
/// they changed, then with an audit entry).
async fn put_settings(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(params): ApiJson<BuyOfferParams>,
) -> Result<Json<BuyHelperSettings>, ApiError> {
    params.validate()?;
    let value = serde_json::to_value(&params)
        .ok()
        .and_then(|v| SettingValue::new(v).ok())
        .ok_or_else(|| invalid_params("The settings cannot be stored"))?;
    let now = chrono::Utc::now();
    state
        .db
        .write(move |conn| {
            in_transaction(conn, |tx| {
                if user_settings::put(tx, user.id, Setting::BuyHelper, &value, now)? {
                    audit::record(
                        tx,
                        now,
                        user.actor(),
                        action::SETTINGS_CHANGED,
                        Some(Setting::BuyHelper.key()),
                        value.value().clone(),
                    )?;
                }
                Ok(())
            })
        })
        .await??;
    Ok(Json(BuyHelperSettings {
        params,
        defaults: defaults(),
    }))
}

#[cfg(test)]
#[path = "buy_helper_tests.rs"]
mod tests;
