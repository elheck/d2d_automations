//! Web server for MTG price tracker UI
//!
//! Provides REST API endpoints for card search and price history visualization.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, Json, Response},
    routing::get,
    Router,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::database::{get_price_history, get_product_by_id, search_products_by_name};
use crate::database::{PriceHistoryPoint, ProductSearchResult};
use crate::image_cache::{fetch_card_info_cached, fetch_image_cached, ImageCache};
use crate::indicators::{calculate_all_indicators, TechnicalIndicators};
use crate::scryfall::CardInfo;

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
    20
}

/// API response wrapper
#[derive(Serialize)]
struct ApiResponse<T> {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Combined price data response
#[derive(Serialize)]
struct PriceData {
    product: ProductSearchResult,
    history: Vec<PriceHistoryPoint>,
    indicators: TechnicalIndicators,
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

/// GET /api/prices/{id_product}
async fn prices_handler(
    State(state): State<AppState>,
    Path(id_product): Path<u64>,
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

    // Get price history
    let history = match get_price_history(&conn, id_product) {
        Ok(h) => h,
        Err(e) => {
            log::error!("Database error: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Calculate technical indicators from trend prices
    let trend_prices: Vec<f64> = history.iter().filter_map(|p| p.trend).collect();
    let indicators = calculate_all_indicators(&trend_prices);

    Ok(Json(ApiResponse {
        success: true,
        data: Some(PriceData {
            product,
            history,
            indicators,
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
        Ok(info) => Ok(Json(ApiResponse {
            success: true,
            data: Some(info),
            error: None,
        })),
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

/// Build the web server router
pub fn create_router(db: Arc<Mutex<Connection>>, image_cache: Arc<ImageCache>) -> Router {
    let state = AppState { db, image_cache };

    Router::new()
        .route("/", get(index_handler))
        .route("/api/search", get(search_handler))
        .route("/api/prices/{id}", get(prices_handler))
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
mod tests {
    use super::*;
    use crate::init_schema;
    use rusqlite::Connection;
    use tempfile::TempDir;

    fn create_test_db() -> (Connection, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = Connection::open(&db_path).unwrap();
        init_schema(&conn).unwrap();
        (conn, temp_dir)
    }

    #[test]
    fn test_create_router() {
        let (conn, temp_dir) = create_test_db();
        let db = Arc::new(Mutex::new(conn));
        let image_cache = Arc::new(ImageCache::new(temp_dir.path()));

        let _router = create_router(db, image_cache);
        // If we got here without panicking, the router was created successfully
    }

    #[test]
    fn test_app_state_clone() {
        let (conn, temp_dir) = create_test_db();
        let db = Arc::new(Mutex::new(conn));
        let image_cache = Arc::new(ImageCache::new(temp_dir.path()));

        let state = AppState {
            db: db.clone(),
            image_cache: image_cache.clone(),
        };

        // Test that AppState is Clone
        let _state2 = state.clone();
    }

    #[test]
    fn test_search_params_default_limit() {
        let params = SearchParams {
            q: "test".to_string(),
            limit: default_limit(),
        };

        assert_eq!(params.limit, 20);
    }

    #[test]
    fn test_api_response_serialization() {
        let response: ApiResponse<Vec<i32>> = ApiResponse {
            success: true,
            data: Some(vec![1, 2, 3]),
            error: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"data\":[1,2,3]"));
    }

    #[test]
    fn test_api_response_error_serialization() {
        let response: ApiResponse<()> = ApiResponse {
            success: false,
            data: None,
            error: Some("Test error".to_string()),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"success\":false"));
        assert!(json.contains("\"error\":\"Test error\""));
        // data should be omitted when None
        assert!(!json.contains("\"data\""));
    }
}
