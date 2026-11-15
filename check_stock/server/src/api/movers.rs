//! Price movers (permission `analysis.view`): cards in stock whose market
//! price moved over 7 or 30 days — spikes to sell into, falling cards to
//! sell off. Built on the shared market data ([`crate::market`]).
//!
//! - `GET /api/movers/status`: the market data job and what is loaded
//! - `POST /api/movers/load` `{refresh}`: loads today's market data (does
//!   nothing when it is there, unless `refresh`)
//! - `GET /api/movers?field=&min_price=&min_age=&direction=&sort=&desc=`

use super::precon_ev::PriceColumn;
use super::{blocking, Access, Api, ApiJson, ApiQuery, CurrentUser};
use crate::app::AppState;
use crate::http::ApiError;
use crate::jobs::JobStatus;
use crate::market::StartError;
use crate::permissions::Permission;
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use check_stock_core::api::cardmarket::product_url;
use check_stock_core::price_trends::{
    build_stock_movers, filter_movers, sort_movers, MoverDirection, MoverSort, StockMover,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Rows sent for the table, as on the desktop.
pub const MOVERS_MAX_ROWS: usize = 300;
pub const MAX_MIN_PRICE: f64 = 1000.0;
pub const MAX_MIN_AGE: i64 = 3650;

pub(super) fn routes(api: Api) -> Api {
    let access = Access::Permission(Permission::AnalysisView);
    api.route("/api/movers/status", access, get(status))
        .route("/api/movers/load", access, post(load))
        .route("/api/movers", access, get(movers))
}

/// What the shared market data job is doing and what is loaded.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MarketStatusView {
    pub configured: bool,
    pub job: JobStatus,
    pub loaded: Option<MarketInfo>,
    /// The stock changed (an import) since the loaded data was read; the
    /// next load reads it again.
    pub stock_changed: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MarketInfo {
    pub version: u64,
    pub loaded_at: String,
    /// The day the prices are for (`YYYY-MM-DD`).
    pub day: String,
    /// Card rows in stock / distinct Cardmarket products asked for.
    pub cards: usize,
    pub products: usize,
}

pub(super) fn market_status(state: &AppState) -> MarketStatusView {
    let market = &state.market;
    MarketStatusView {
        configured: market.is_configured(),
        job: market.status(),
        loaded: market.loaded().map(|d| MarketInfo {
            version: d.version,
            loaded_at: crate::migrations::format_utc(d.loaded_at),
            day: d.today.format("%Y-%m-%d").to_string(),
            cards: d.cards.len(),
            products: d.products,
        }),
        stock_changed: market.is_outdated(),
    }
}

async fn status(State(state): State<AppState>) -> Json<MarketStatusView> {
    Json(market_status(&state))
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct MarketLoadRequest {
    /// Load again even if today's data is there.
    pub refresh: bool,
}

/// Starts (or skips) a load of the market data and answers with the status.
pub(super) async fn start_market_load(
    state: &AppState,
    user: &CurrentUser,
    refresh: bool,
) -> Result<Response, ApiError> {
    match state.market.start_load(state.db.clone(), refresh) {
        Ok(true) => log::info!(
            "Market data load started by {} (refresh: {refresh})",
            user.username
        ),
        Ok(false) | Err(StartError::Running) => {}
        Err(StartError::NotConfigured) => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "not_configured",
                "No price tracker is configured on the server",
            ))
        }
        Err(StartError::TooSoon { wait }) => {
            let seconds = wait.as_secs().max(1);
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "too_soon",
                format!("The market data was just loaded; try again in {seconds} s"),
            )
            .with_header(
                header::RETRY_AFTER,
                HeaderValue::from_str(&seconds.to_string()).map_err(|_| ApiError::internal())?,
            ));
        }
    }
    Ok((StatusCode::ACCEPTED, Json(market_status(state))).into_response())
}

async fn load(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<MarketLoadRequest>,
) -> Result<Response, ApiError> {
    start_market_load(&state, &user, request.refresh).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MoverDirectionKey {
    All,
    Risers,
    Fallers,
}

impl From<MoverDirectionKey> for MoverDirection {
    fn from(d: MoverDirectionKey) -> Self {
        match d {
            MoverDirectionKey::All => MoverDirection::All,
            MoverDirectionKey::Risers => MoverDirection::Risers,
            MoverDirectionKey::Fallers => MoverDirection::Fallers,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MoverSortKey {
    Change7,
    Change30,
    Age,
    Current,
    Listed,
    Name,
    Set,
    Quantity,
    Location,
}

impl From<MoverSortKey> for MoverSort {
    fn from(s: MoverSortKey) -> Self {
        match s {
            MoverSortKey::Change7 => MoverSort::Change7,
            MoverSortKey::Change30 => MoverSort::Change30,
            MoverSortKey::Age => MoverSort::Age,
            MoverSortKey::Current => MoverSort::Current,
            MoverSortKey::Listed => MoverSort::Listed,
            MoverSortKey::Name => MoverSort::Name,
            MoverSortKey::Set => MoverSort::Set,
            MoverSortKey::Quantity => MoverSort::Quantity,
            MoverSortKey::Location => MoverSort::Location,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MoversQuery {
    field: PriceColumn,
    min_price: f64,
    min_age: i64,
    direction: MoverDirectionKey,
    sort: MoverSortKey,
    desc: bool,
}

/// One card in stock whose market price moved.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct MoverRow {
    pub name: String,
    pub set_code: String,
    pub condition: String,
    pub language: String,
    pub is_foil: bool,
    pub quantity: i64,
    /// Days since it was listed.
    pub age_days: i64,
    /// Our listing price (euros).
    pub listed: f64,
    /// Market value now, in the chosen price field.
    pub market: Option<f64>,
    pub pct_7d: Option<f64>,
    pub pct_30d: Option<f64>,
    pub location: String,
    pub cardmarket_url: Option<String>,
}

impl From<&StockMover> for MoverRow {
    fn from(m: &StockMover) -> Self {
        let c = &m.card;
        Self {
            name: c.name.clone(),
            set_code: c.set_code.clone(),
            condition: c.condition.clone(),
            language: c.language.clone(),
            is_foil: c.is_foil,
            quantity: c.quantity,
            age_days: m.age_days,
            listed: c.price,
            market: m.change.current,
            pct_7d: m.change.pct_7d,
            pct_30d: m.change.pct_30d,
            location: c.location.clone(),
            cardmarket_url: c.cardmarket_id.trim().parse::<u64>().ok().map(product_url),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MoversView {
    /// Version of the market data used.
    pub version: u64,
    pub rows: Vec<MoverRow>,
    pub total: usize,
}

fn invalid_params(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_params", message)
}

async fn movers(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<MoversQuery>,
) -> Result<Json<MoversView>, ApiError> {
    if !query.min_price.is_finite() || !(0.0..=MAX_MIN_PRICE).contains(&query.min_price) {
        return Err(invalid_params(
            "The minimum price must be between 0 and 1000 €",
        ));
    }
    if !(0..=MAX_MIN_AGE).contains(&query.min_age) {
        return Err(invalid_params(
            "The minimum age must be between 0 and 3650 days",
        ));
    }
    let data = state.market.loaded().ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "not_loaded",
            "The market data is not loaded yet",
        )
    })?;
    let view = blocking(move || {
        let movers = build_stock_movers(
            &data.cards,
            &data.snapshots,
            query.field.into(),
            data.today,
            query.min_price,
        );
        let mut movers = filter_movers(movers, query.min_age, query.direction.into());
        sort_movers(&mut movers, query.sort.into(), query.desc);
        MoversView {
            version: data.version,
            total: movers.len(),
            rows: movers
                .iter()
                .take(MOVERS_MAX_ROWS)
                .map(MoverRow::from)
                .collect(),
        }
    })
    .await?;
    Ok(Json(view))
}

#[cfg(test)]
#[path = "movers_tests.rs"]
mod tests;
