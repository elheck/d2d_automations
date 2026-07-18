//! Web server for MTG price tracker UI
//!
//! Provides REST API endpoints for card search and price history visualization.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, Json, Response},
    routing::{get, post},
    Router,
};
use rusqlite::Connection;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

use crate::database::{
    get_id_expansion_for_product, get_latest_prices_bulk, get_price_history,
    get_price_snapshots_bulk, get_product_by_id, search_products_by_name, upsert_expansion_name,
};
use crate::database::{LatestPrice, PriceSnapshot, ProductSearchResult};
use crate::image_cache::{fetch_card_info_cached, fetch_image_cached, ImageCache};
use crate::indicators::{calculate_all_indicators, calculate_cardmarket_signals};
use crate::scryfall::CardInfo;
use mtg_common::inventory_sync::{
    ApiResponse, BulkPriceRequest, PriceData, PriceSnapshotRequest, MAX_BULK_IDS,
    MAX_SNAPSHOT_DATES,
};

/// Shared application state (thread-safe database connection + image cache)
#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    image_cache: Arc<ImageCache>,
}

/// Search query parameters
#[derive(Deserialize)]
struct SearchParams {
    q: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    100
}

/// Price history query parameters
///
/// Supports two optional filters (if both provided, `since` takes precedence):
/// - `days=N` — last N days of history
/// - `since=YYYY-MM-DD` — history on or after this date
#[derive(Deserialize, Default)]
struct PriceParams {
    days: Option<u32>,
    since: Option<String>,
}

impl PriceParams {
    /// Resolve to an ISO date string cut-off, or `None` for full history.
    fn since_date(&self) -> Option<String> {
        if let Some(ref date) = self.since {
            return Some(date.clone());
        }
        if let Some(days) = self.days {
            let cutoff = chrono::Utc::now()
                .date_naive()
                .checked_sub_days(chrono::Days::new(u64::from(days)))?;
            return Some(cutoff.format("%Y-%m-%d").to_string());
        }
        None
    }
}

/// GET /api/health - Simple connectivity check
async fn health_handler() -> Json<ApiResponse<&'static str>> {
    Json(ApiResponse {
        success: true,
        data: Some("ok"),
        error: None,
    })
}

/// GET / - Serve the web UI (single HTML page)
async fn index_handler() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

/// GET /api/search?q={query}&limit={limit}
async fn search_handler(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<ApiResponse<Vec<ProductSearchResult>>>, StatusCode> {
    let conn = state.db.lock().unwrap();

    match search_products_by_name(&conn, &params.q, params.limit) {
        Ok(results) => Ok(Json(ApiResponse {
            success: true,
            data: Some(results),
            error: None,
        })),
        Err(e) => {
            log::error!("Search error: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// GET /api/prices/{id_product}?days=90
/// GET /api/prices/{id_product}?since=2025-01-01
async fn prices_handler(
    State(state): State<AppState>,
    Path(id_product): Path<u64>,
    Query(params): Query<PriceParams>,
) -> Result<Json<ApiResponse<PriceData>>, StatusCode> {
    let conn = state.db.lock().unwrap();

    // Get product details
    let product = match get_product_by_id(&conn, id_product) {
        Ok(Some(p)) => p,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            log::error!("Database error: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let since_date = params.since_date();

    // Get price history
    let history = match get_price_history(&conn, id_product, since_date.as_deref()) {
        Ok(h) => h,
        Err(e) => {
            log::error!("Database error: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Calculate technical indicators from trend prices
    let trend_prices: Vec<f64> = history.iter().filter_map(|p| p.trend).collect();
    let indicators = calculate_all_indicators(&trend_prices);

    // Cardmarket-native signals — operate on the full history (preserving None alignment)
    let avg1: Vec<Option<f64>> = history.iter().map(|p| p.avg1).collect();
    let avg7: Vec<Option<f64>> = history.iter().map(|p| p.avg7).collect();
    let avg30: Vec<Option<f64>> = history.iter().map(|p| p.avg30).collect();
    let low: Vec<Option<f64>> = history.iter().map(|p| p.low).collect();
    let trend: Vec<Option<f64>> = history.iter().map(|p| p.trend).collect();
    let cardmarket_signals = calculate_cardmarket_signals(&avg1, &avg7, &avg30, &low, &trend);

    Ok(Json(ApiResponse {
        success: true,
        data: Some(PriceData {
            product,
            history,
            indicators,
            cardmarket_signals,
        }),
        error: None,
    }))
}

/// GET /api/card-image/{id_product}
/// Fetches and caches card images from Scryfall using Cardmarket product ID
async fn card_image_handler(
    State(state): State<AppState>,
    Path(id_product): Path<u64>,
) -> Response {
    match fetch_image_cached(&state.image_cache, id_product).await {
        Ok(image_bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/jpeg")
            .header(header::CACHE_CONTROL, "public, max-age=86400")
            .body(Body::from(image_bytes))
            .unwrap(),
        Err(e) => {
            log::warn!("Failed to fetch image for product {}: {}", id_product, e);
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from(format!("Image not found: {}", e)))
                .unwrap()
        }
    }
}

/// GET /api/card-info/{id_product}
/// Returns cached Scryfall metadata (set name, type, mana cost, rarity, oracle text, purchase links)
async fn card_info_handler(
    State(state): State<AppState>,
    Path(id_product): Path<u64>,
) -> Result<Json<ApiResponse<CardInfo>>, StatusCode> {
    match fetch_card_info_cached(&state.image_cache, id_product).await {
        Ok(info) => {
            // Populate expansion name cache from successful Scryfall lookup
            if let Some(ref set_name) = info.set_name {
                let conn = state.db.lock().unwrap();
                if let Ok(Some(id_expansion)) = get_id_expansion_for_product(&conn, id_product) {
                    let _ = upsert_expansion_name(&conn, id_expansion, set_name);
                }
            }
            Ok(Json(ApiResponse {
                success: true,
                data: Some(info),
                error: None,
            }))
        }
        Err(e) => {
            log::warn!(
                "Failed to fetch card info for product {}: {}",
                id_product,
                e
            );
            Err(StatusCode::NOT_FOUND)
        }
    }
}

/// POST /api/latest-prices
/// Returns the most recent price row for each requested product ID.
async fn latest_prices_handler(
    State(state): State<AppState>,
    Json(body): Json<BulkPriceRequest>,
) -> Result<Json<ApiResponse<Vec<LatestPrice>>>, StatusCode> {
    if body.ids.len() > MAX_BULK_IDS {
        return Ok(Json(ApiResponse::err(format!(
            "Too many IDs (max {MAX_BULK_IDS})"
        ))));
    }
    let conn = state.db.lock().unwrap();
    match get_latest_prices_bulk(&conn, &body.ids) {
        Ok(prices) => Ok(Json(ApiResponse {
            success: true,
            data: Some(prices),
            error: None,
        })),
        Err(e) => {
            log::error!("Bulk price lookup error: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// POST /api/price-snapshots
///
/// Returns, for each (product ID, date) pair, the price row in effect on that
/// date (most recent row on or before it). This is a pure indexed lookup —
/// no aggregation happens server-side; clients compute deltas themselves.
async fn price_snapshots_handler(
    State(state): State<AppState>,
    Json(body): Json<PriceSnapshotRequest>,
) -> Result<Json<ApiResponse<Vec<PriceSnapshot>>>, StatusCode> {
    if body.ids.len() > MAX_BULK_IDS {
        return Ok(Json(ApiResponse::err(format!(
            "Too many IDs (max {MAX_BULK_IDS})"
        ))));
    }
    if body.dates.len() > MAX_SNAPSHOT_DATES {
        return Ok(Json(ApiResponse::err(format!(
            "Too many dates (max {MAX_SNAPSHOT_DATES})"
        ))));
    }
    // Dates reach parameterized SQL as-is; validate the shape anyway so
    // malformed input fails loudly instead of silently matching nothing.
    for date in &body.dates {
        if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
            return Ok(Json(ApiResponse::err(format!(
                "Invalid date '{date}' (expected YYYY-MM-DD)"
            ))));
        }
    }
    let conn = state.db.lock().unwrap();
    match get_price_snapshots_bulk(&conn, &body.ids, &body.dates) {
        Ok(snapshots) => Ok(Json(ApiResponse::ok(snapshots))),
        Err(e) => {
            log::error!("Price snapshot lookup error: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Build the web server router
pub fn create_router(db: Arc<Mutex<Connection>>, image_cache: Arc<ImageCache>) -> Router {
    let state = AppState { db, image_cache };

    Router::new()
        .route("/", get(index_handler))
        .route("/api/health", get(health_handler))
        .route("/api/search", get(search_handler))
        .route("/api/prices/{id}", get(prices_handler))
        .route("/api/latest-prices", post(latest_prices_handler))
        .route("/api/price-snapshots", post(price_snapshots_handler))
        .route("/api/card-image/{id}", get(card_image_handler))
        .route("/api/card-info/{id}", get(card_info_handler))
        .with_state(state)
}

/// Start the web server (async)
///
/// Binds to 0.0.0.0 (all interfaces) to work with Docker port mapping.
/// When running locally, use firewall rules to restrict access.
/// When running in Docker, use port mapping to control external exposure.
pub async fn serve(
    db: Arc<Mutex<Connection>>,
    db_path: &std::path::Path,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create image cache in the same directory as the database
    let db_dir = db_path.parent().ok_or("Failed to get database directory")?;
    let image_cache = Arc::new(ImageCache::new(db_dir));

    let app = create_router(db, image_cache);
    let addr = format!("0.0.0.0:{}", port);

    log::info!("Web UI listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
#[path = "web_tests.rs"]
mod tests;
