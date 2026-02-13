//! Web server for MTG price tracker UI
//!
//! Provides REST API endpoints for card search and price history visualization.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, Json},
    routing::get,
    Router,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::database::{get_price_history, get_product_by_id, search_products_by_name};
use crate::database::{PriceHistoryPoint, ProductSearchResult};

/// Shared application state (thread-safe database connection)
#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
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

    Ok(Json(ApiResponse {
        success: true,
        data: Some(PriceData { product, history }),
        error: None,
    }))
}

/// Build the web server router
pub fn create_router(db: Arc<Mutex<Connection>>) -> Router {
    let state = AppState { db };

    Router::new()
        .route("/", get(index_handler))
        .route("/api/search", get(search_handler))
        .route("/api/prices/{id}", get(prices_handler))
        .with_state(state)
}

/// Start the web server (async)
///
/// Binds to 0.0.0.0 (all interfaces) to work with Docker port mapping.
/// When running locally, use firewall rules to restrict access.
/// When running in Docker, use port mapping to control external exposure.
pub async fn serve(
    db: Arc<Mutex<Connection>>,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let app = create_router(db);
    let addr = format!("0.0.0.0:{}", port);

    log::info!("Web UI listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
