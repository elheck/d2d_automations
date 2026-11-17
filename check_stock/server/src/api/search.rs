//! Search (`search.use`) and discards (`inventory.discard`) on the stock of
//! the last import minus pending discards ([`crate::stock`]).
//!
//! - `GET /api/search?q=&fields=&case_sensitive=&all_languages=&page=&per_page=`
//! - `GET /api/search/history/{product}?foil=`: price history from the
//!   tracker, only for products in the stock (the server is no open relay).
//! - `POST /api/inventory/discard` `{items: [{pile, quantity}]}`: writes the
//!   piles off (rules in [`crate::discards`]).
//! - `GET /api/inventory/discards`, `GET /api/inventory/discards/{id}/csv`:
//!   the history and the stock-update CSV (exact values: TCG Powertools
//!   imports it).

use super::bins::source;
use super::{blocking, Access, Api, ApiJson, ApiQuery, CurrentUser};
use crate::app::AppState;
use crate::discards::{self, recent, DiscardError, DiscardSummary, MAX_ITEMS};
use crate::http::ApiError;
use crate::inventory_import::{latest_version, Importer};
use crate::permissions::Permission;
use crate::stock::quantity;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use check_stock_core::api::cardmarket::product_url;
use check_stock_core::api::inventory_sync::{PriceField, PriceFields};
use check_stock_core::price_trends::roc_from_history;
use check_stock_core::search::{card_matches, prepare_term, SearchFields, SearchOptions};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use ts_rs::TS;

/// Longest search text.
const MAX_QUERY_CHARS: usize = 200;
/// Page sizes offered.
const PAGE_SIZES: [usize; 4] = [25, 50, 100, 200];
/// Days of price history shown.
const HISTORY_DAYS: u32 = 120;
/// How long a history request to the tracker may take.
const HISTORY_TIMEOUT: Duration = Duration::from_secs(10);
/// Discards listed in the history.
const DISCARD_HISTORY: u32 = 20;

pub(super) fn routes(api: Api) -> Api {
    api.route(
        "/api/search",
        Access::Permission(Permission::SearchUse),
        get(search),
    )
    .route(
        "/api/search/history/{product}",
        Access::Permission(Permission::SearchUse),
        get(history),
    )
    .route(
        "/api/inventory/discard",
        Access::Permission(Permission::InventoryDiscard),
        post(discard),
    )
    .route(
        "/api/inventory/discards",
        Access::Permission(Permission::InventoryDiscard),
        get(discard_history),
    )
    .route(
        "/api/inventory/discards/{id}/csv",
        Access::Permission(Permission::InventoryDiscard),
        get(discard_csv),
    )
}

fn no_import() -> ApiError {
    ApiError::conflict(
        "no_import",
        "Search needs an inventory import on the server (Tools → Inventory import)",
    )
}

// ── Search ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
    /// Comma-separated: name, set, condition, language, location, rarity,
    /// price, comment.
    pub fields: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub all_languages: bool,
    #[serde(default)]
    pub page: usize,
    pub per_page: usize,
}

fn options(query: &SearchQuery) -> Result<SearchOptions, ApiError> {
    let mut fields = SearchFields {
        name: false,
        set: false,
        ..SearchFields::default()
    };
    for field in query.fields.split(',').filter(|f| !f.is_empty()) {
        match field {
            "name" => fields.name = true,
            "set" => fields.set = true,
            "condition" => fields.condition = true,
            "language" => fields.language = true,
            "location" => fields.location = true,
            "rarity" => fields.rarity = true,
            "price" => fields.price = true,
            "comment" => fields.comment = true,
            _ => return Err(ApiError::bad_request("Unknown search field")),
        }
    }
    Ok(SearchOptions {
        fields,
        case_sensitive: query.case_sensitive,
        all_languages: query.all_languages,
    })
}

/// One pile of the stock.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct PileRow {
    /// Pile id (for discards and lists).
    pub id: String,
    pub name: String,
    pub set_name: String,
    pub set_code: String,
    pub collector_number: String,
    pub condition: String,
    pub language: String,
    pub foil: bool,
    pub signed: bool,
    pub quantity: i64,
    /// Listed price per copy.
    pub price: f64,
    pub location: String,
    pub rarity: String,
    pub comment: String,
    pub product_id: Option<u64>,
    pub cardmarket_url: Option<String>,
}

pub(super) fn pile_row(id: &str, c: &check_stock_core::models::Card) -> PileRow {
    let product_id = c.cardmarket_id.trim().parse::<u64>().ok();
    PileRow {
        id: id.to_string(),
        name: c.name.clone(),
        set_name: c.set.clone(),
        set_code: c.set_code.clone(),
        collector_number: c.cn.clone(),
        condition: c.condition.clone(),
        language: c.language.clone(),
        foil: c.is_foil_card(),
        signed: c.is_signed_card(),
        quantity: quantity(c),
        price: c.price_f64(),
        location: c.location.clone().unwrap_or_default(),
        rarity: c.rarity.clone(),
        comment: c.comment.clone(),
        product_id,
        cardmarket_url: product_id.map(product_url),
    }
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SearchView {
    pub source: Option<super::bins::SourceInfo>,
    /// All matching piles.
    pub total: i64,
    /// Copies in them.
    pub copies: i64,
    pub page: i64,
    pub per_page: i64,
    pub rows: Vec<PileRow>,
}

async fn search(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<SearchQuery>,
) -> Result<Json<SearchView>, ApiError> {
    if query.q.chars().count() > MAX_QUERY_CHARS {
        return Err(ApiError::bad_request("The search text is too long"));
    }
    if !PAGE_SIZES.contains(&query.per_page) {
        return Err(ApiError::bad_request("per_page must be 25, 50, 100 or 200"));
    }
    let options = options(&query)?;
    let Some(source) = source(&state).await? else {
        return Ok(Json(SearchView {
            source: None,
            total: 0,
            copies: 0,
            page: 0,
            per_page: query.per_page as i64,
            rows: Vec::new(),
        }));
    };
    let view = blocking(move || {
        let term = prepare_term(query.q.trim(), &options);
        let hits: Vec<usize> = (0..source.cards.len())
            .filter(|&i| card_matches(&source.cards[i], &term, &options))
            .collect();
        let copies = hits.iter().map(|&i| quantity(&source.cards[i])).sum();
        let rows = hits
            .iter()
            .skip(query.page.saturating_mul(query.per_page))
            .take(query.per_page)
            .map(|&i| pile_row(&source.ids[i], &source.cards[i]))
            .collect();
        SearchView {
            source: Some(super::bins::SourceInfo::from(source.as_ref())),
            total: hits.len() as i64,
            copies,
            page: query.page as i64,
            per_page: query.per_page as i64,
            rows,
        }
    })
    .await?;
    Ok(Json(view))
}

// ── Price history ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryQuery {
    #[serde(default)]
    pub foil: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct CardHistoryPoint {
    pub date: String,
    pub price: f64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct CardHistory {
    pub name: String,
    pub expansion: Option<String>,
    pub foil: bool,
    /// Daily trend prices (foil column for foils).
    pub points: Vec<CardHistoryPoint>,
    pub trend: Option<f64>,
    pub change_7d: Option<f64>,
    pub change_30d: Option<f64>,
}

async fn history(
    State(state): State<AppState>,
    Path(product): Path<u64>,
    ApiQuery(query): ApiQuery<HistoryQuery>,
) -> Result<Json<CardHistory>, ApiError> {
    let source = source(&state).await?.ok_or_else(no_import)?;
    if !source.has_product(product) {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_in_stock",
            "That card is not in the stock",
        ));
    }
    let unavailable = || {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "tracker_unavailable",
            "The price tracker cannot be reached right now",
        )
    };
    let tracker = state.inventory_sync.clone().ok_or_else(unavailable)?;
    let data = match tokio::time::timeout(
        HISTORY_TIMEOUT,
        tracker.price_history(product, Some(HISTORY_DAYS)),
    )
    .await
    {
        Ok(Ok(data)) => data,
        Ok(Err(e)) => {
            log::warn!("price history of {product} failed: {e}");
            return Err(unavailable());
        }
        Err(_) => {
            log::warn!("price history of {product} timed out");
            return Err(unavailable());
        }
    };
    let foil = query.foil;
    let points: Vec<CardHistoryPoint> = data
        .history
        .iter()
        .filter_map(|p| {
            p.price_for(PriceField::Trend, foil)
                .map(|price| CardHistoryPoint {
                    date: p.price_date.clone(),
                    price,
                })
        })
        .collect();
    Ok(Json(CardHistory {
        name: data.product.name.clone(),
        expansion: data.product.expansion_name.clone(),
        foil,
        trend: points.last().map(|p| p.price),
        change_7d: roc_from_history(&data.history, 7, foil),
        change_30d: roc_from_history(&data.history, 30, foil),
        points,
    }))
}

// ── Discards ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct DiscardItem {
    pub pile: String,
    pub quantity: i64,
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct DiscardRequest {
    pub items: Vec<DiscardItem>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiscardResult {
    pub id: i64,
    pub copies: i64,
    pub variants: i64,
    /// Listed value of the copies (only with `financials.view`).
    pub value: Option<f64>,
    /// The data version after the discard.
    pub data_version: i64,
}

fn discard_error(e: DiscardError) -> ApiError {
    match e {
        DiscardError::Empty | DiscardError::TooMany | DiscardError::BadQuantity => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_discard",
            "Discard 1 to 1,000 piles, each with 1 to 1,000,000 copies",
        ),
        DiscardError::UnknownPile | DiscardError::Changed => ApiError::conflict(
            "stock_changed",
            "The stock changed since the search (a discard or an import); search again",
        ),
        DiscardError::NotEnough { name, available } => ApiError::conflict(
            "not_enough",
            format!(
                "Only {available} {} of \"{}\" {} left",
                if available == 1 { "copy" } else { "copies" },
                name.chars().take(60).collect::<String>(),
                if available == 1 { "is" } else { "are" },
            ),
        ),
        DiscardError::BackupFailed => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "backup_failed",
            "The backup before the discard failed, so nothing was discarded. Try again; if it keeps failing, the server's disk may be full",
        ),
    }
}

async fn discard(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<DiscardRequest>,
) -> Result<Json<DiscardResult>, ApiError> {
    if request.items.is_empty()
        || request.items.len() > MAX_ITEMS
        || request.items.iter().any(|i| i.pile.len() > 24)
    {
        return Err(discard_error(DiscardError::TooMany));
    }
    let source = source(&state).await?.ok_or_else(no_import)?;
    let items: Vec<(String, i64)> = request
        .items
        .into_iter()
        .map(|i| (i.pile, i.quantity))
        .collect();
    let db_path = state.db.path().to_path_buf();
    let (id, username, display_name) = (user.id, user.username.clone(), user.display_name.clone());
    let result = state
        .db
        .write(move |conn| {
            let done = discards::discard(
                conn,
                &db_path,
                Importer {
                    id,
                    username: &username,
                    display_name: &display_name,
                },
                &source,
                &items,
                Utc::now(),
            )?;
            Ok(match done {
                Ok(done) => Ok((done, latest_version(conn)?.0)),
                Err(e) => Err(e),
            })
        })
        .await?;
    let (done, data_version) = result.map_err(discard_error)?;
    state.market.stock_changed();
    let money = user.permissions.contains(Permission::FinancialsView);
    Ok(Json(DiscardResult {
        id: done.id,
        copies: done.copies,
        variants: done.variants,
        value: money.then_some(done.value),
        data_version,
    }))
}

async fn discard_history(
    State(state): State<AppState>,
) -> Result<Json<Vec<DiscardSummary>>, ApiError> {
    Ok(Json(
        state.db.read(|conn| recent(conn, DISCARD_HISTORY)).await?,
    ))
}

async fn discard_csv(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let csv = state
        .db
        .read(move |conn| discards::csv_of(conn, id))
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "That discard's CSV is no longer kept (only the newest 20 are)",
            )
        })?;
    let disposition = format!("attachment; filename=\"discarded_cards_{id}.csv\"");
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/csv; charset=utf-8"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&disposition).expect("ascii"),
            ),
        ],
        csv,
    )
        .into_response())
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
