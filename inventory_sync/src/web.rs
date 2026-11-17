//! Web server for the MTG price tracker: its page and a read-only JSON API.
//!
//! The tracker is open on the home LAN without login (owner's decision,
//! 2026-11-14: its own page calls the API from the browser, and its data is
//! market prices). So it must stay read-only and protect itself:
//! - searches are capped ([`MAX_SEARCH_LIMIT`] results, text length), dates
//!   are checked;
//! - Scryfall is asked only about products in the database, one request at a
//!   time ([`Scryfall`]), and misses are remembered for 30 days;
//! - per-client rate limits for all requests and for Scryfall lookups
//!   ([`RateLimiter`]); `X-Forwarded-For` only counts from trusted proxies;
//! - generic error messages, a poisoned database lock never takes the API
//!   down, a panic becomes a 500, requests time out after
//!   [`REQUEST_TIMEOUT`], bodies are at most [`BODY_LIMIT`];
//! - security headers on every response;
//! - loopback only unless configured ([`WebConfig::bind`]); an optional `Host`
//!   allowlist against DNS rebinding.

use axum::body::Body;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Extension, Path, Query, Request, State};
use axum::http::{header, HeaderName, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use mtg_common::net::{HostAllowlist, TrustedProxies};
use rusqlite::Connection;
use serde::Deserialize;
use std::any::Any;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;

use crate::database::{
    get_id_expansion_for_product, get_latest_prices_bulk, get_price_history,
    get_price_snapshots_bulk, get_product_by_id, search_products_by_name, upsert_expansion_name,
};
use crate::image_cache::{
    fetch_card_info_cached, fetch_image_cached, is_permanent_miss, ImageCache,
};
use crate::indicators::{calculate_all_indicators, calculate_cardmarket_signals};
use crate::rate_limit::{retry_after_secs, RateLimiter};
use crate::scryfall::{image_type, Scryfall};
use crate::InventoryError;
use mtg_common::inventory_sync::{
    ApiResponse, BulkPriceRequest, PriceData, PriceSnapshotRequest, MAX_BULK_IDS, MAX_SEARCH_LIMIT,
    MAX_SEARCH_QUERY_CHARS, MAX_SNAPSHOT_DATES,
};

/// Longest a request may take.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Largest request body (10,000 product ids are about 100 KB of JSON).
pub const BODY_LIMIT: usize = 512 * 1024;
/// Listen address unless configured: this machine only.
pub const DEFAULT_BIND: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// The tracker page has an inline script and inline styles and loads
/// Chart.js (with an integrity hash) and Google Fonts; nothing else.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; \
    script-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net; \
    style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
    font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; \
    base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// Sent with every response, errors included.
pub const SECURITY_HEADERS: [(&str, &str); 7] = [
    ("content-security-policy", CONTENT_SECURITY_POLICY),
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "DENY"),
    ("referrer-policy", "no-referrer"),
    ("cross-origin-opener-policy", "same-origin"),
    ("cross-origin-resource-policy", "same-origin"),
    (
        "permissions-policy",
        "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
    ),
];

/// How the web server is reached and protected.
pub struct WebOptions {
    /// Proxies whose `X-Forwarded-For` is believed (Caddy).
    pub trusted_proxies: TrustedProxies,
    /// `None`: the `Host` header is not checked.
    pub allowed_hosts: Option<HostAllowlist>,
    pub scryfall: Arc<Scryfall>,
    /// All requests, per client.
    pub requests: Arc<RateLimiter>,
    /// Lookups that reach Scryfall, per client.
    pub scryfall_lookups: Arc<RateLimiter>,
}

impl Default for WebOptions {
    fn default() -> Self {
        Self {
            trusted_proxies: TrustedProxies::default(),
            allowed_hosts: None,
            scryfall: Arc::new(Scryfall::default()),
            requests: Arc::new(RateLimiter::requests()),
            scryfall_lookups: Arc::new(RateLimiter::scryfall()),
        }
    }
}

/// Where the web server listens, and its options.
pub struct WebConfig {
    pub bind: IpAddr,
    pub port: u16,
    pub options: WebOptions,
}

/// Shared application state.
#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    image_cache: Arc<ImageCache>,
    scryfall: Arc<Scryfall>,
    scryfall_lookups: Arc<RateLimiter>,
}

/// The client's address (after trusted proxies), set by [`limit_requests`].
#[derive(Clone, Copy, Debug)]
struct ClientIp(IpAddr);

struct ClientLimits {
    proxies: TrustedProxies,
    requests: Arc<RateLimiter>,
}

/// The database, also after a panic elsewhere poisoned the lock (the
/// connection itself is fine: a panicking transaction rolls back).
fn lock(db: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(PoisonError::into_inner)
}

// ── Responses ──────────────────────────────────────────────────────────────

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(ApiResponse::<()>::err(message))).into_response()
}

fn bad_request(message: impl Into<String>) -> Response {
    error(StatusCode::BAD_REQUEST, message)
}

/// A body that could not be read: too large (413) or not the expected JSON
/// (400); never the parser's text, which can quote the body.
fn body_rejected(rejection: JsonRejection) -> Response {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        error(StatusCode::PAYLOAD_TOO_LARGE, "The request is too large")
    } else {
        bad_request("Invalid request body")
    }
}

fn not_found(message: &str) -> Response {
    error(StatusCode::NOT_FOUND, message)
}

fn internal() -> Response {
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal error, see the server log",
    )
}

fn too_many(wait: Duration) -> Response {
    let secs = retry_after_secs(wait);
    let mut response = error(
        StatusCode::TOO_MANY_REQUESTS,
        format!("Too many requests; try again in {secs} s"),
    );
    if let Ok(value) = HeaderValue::from_str(&secs.to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

fn panic_response(panic: Box<dyn Any + Send + 'static>) -> Response<Body> {
    let message = panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    log::error!("request handler panicked: {message}");
    internal()
}

// ── Middleware ─────────────────────────────────────────────────────────────

/// Works out the client's address and applies its request limit.
async fn limit_requests(
    State(limits): State<Arc<ClientLimits>>,
    mut request: Request,
    next: Next,
) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |c| c.0.ip());
    let forwarded = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok());
    let client = limits.proxies.client_ip(peer, forwarded);
    if let Err(wait) = limits.requests.check(client, Instant::now()) {
        return too_many(wait);
    }
    request.extensions_mut().insert(ClientIp(client));
    next.run(request).await
}

/// Refuses requests for host names the server does not answer to.
async fn check_host(
    State(allowed): State<Arc<HostAllowlist>>,
    request: Request,
    next: Next,
) -> Response {
    let header_host = request.headers().get(header::HOST).map(|v| v.to_str());
    let uri_host = request.uri().authority().map(|a| a.as_str());
    let allowed_request = match (header_host, uri_host) {
        (None, None) | (Some(Err(_)), _) => false,
        (Some(Ok(h)), None) => allowed.allows(h),
        (None, Some(u)) => allowed.allows(u),
        (Some(Ok(h)), Some(u)) => allowed.allows(h) && allowed.allows(u),
    };
    if allowed_request {
        next.run(request).await
    } else {
        bad_request("Unknown host name")
    }
}

// ── Handlers ───────────────────────────────────────────────────────────────

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
    Json(ApiResponse::ok("ok"))
}

/// GET / - Serve the web UI (single HTML page)
async fn index_handler() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

/// GET /api/search?q={query}&limit={limit}
async fn search_handler(
    State(state): State<AppState>,
    params: Result<Query<SearchParams>, QueryRejection>,
) -> Response {
    let Ok(Query(params)) = params else {
        return bad_request("Invalid search parameters");
    };
    if params.q.chars().count() > MAX_SEARCH_QUERY_CHARS {
        return bad_request(format!(
            "The search text is too long (at most {MAX_SEARCH_QUERY_CHARS} characters)"
        ));
    }
    let limit = params.limit.clamp(1, MAX_SEARCH_LIMIT);
    let conn = lock(&state.db);
    match search_products_by_name(&conn, &params.q, limit) {
        Ok(results) => Json(ApiResponse::ok(results)).into_response(),
        Err(e) => {
            log::error!("Search error: {e}");
            internal()
        }
    }
}

/// GET /api/prices/{id_product}?days=90
/// GET /api/prices/{id_product}?since=2025-01-01
async fn prices_handler(
    State(state): State<AppState>,
    id: Result<Path<u64>, PathRejection>,
    params: Result<Query<PriceParams>, QueryRejection>,
) -> Response {
    let (Ok(Path(id_product)), Ok(Query(params))) = (id, params) else {
        return bad_request("Invalid product id or parameters");
    };
    if let Some(since) = &params.since {
        if chrono::NaiveDate::parse_from_str(since, "%Y-%m-%d").is_err() {
            return bad_request("Invalid date (expected YYYY-MM-DD)");
        }
    }
    let conn = lock(&state.db);
    let product = match get_product_by_id(&conn, id_product) {
        Ok(Some(p)) => p,
        Ok(None) => return not_found("Unknown product"),
        Err(e) => {
            log::error!("Database error: {e}");
            return internal();
        }
    };
    let since_date = params.since_date();
    let history = match get_price_history(&conn, id_product, since_date.as_deref()) {
        Ok(h) => h,
        Err(e) => {
            log::error!("Database error: {e}");
            return internal();
        }
    };
    drop(conn);

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

    Json(ApiResponse::ok(PriceData {
        product,
        history,
        indicators,
        cardmarket_signals,
    }))
    .into_response()
}

/// Why Scryfall may not be asked about `id` for this client, if it may not:
/// only products in the database, not a recent miss, within the client's
/// lookup budget.
fn scryfall_refusal(state: &AppState, client: IpAddr, id: u64) -> Option<Response> {
    match get_product_by_id(&lock(&state.db), id) {
        Ok(Some(_)) => {}
        Ok(None) => return Some(not_found("Unknown product")),
        Err(e) => {
            log::error!("Database error: {e}");
            return Some(internal());
        }
    }
    if state.image_cache.is_recent_miss(id, SystemTime::now()) {
        return Some(not_found("Scryfall has nothing for this product"));
    }
    state
        .scryfall_lookups
        .check(client, Instant::now())
        .err()
        .map(too_many)
}

fn lookup_failed(id: u64, e: &InventoryError) -> Response {
    if is_permanent_miss(e) {
        not_found("Scryfall has nothing for this product")
    } else {
        log::warn!("Scryfall lookup for product {id} failed: {e}");
        error(
            StatusCode::BAD_GATEWAY,
            "Scryfall cannot be reached right now",
        )
    }
}

fn image_response(bytes: Vec<u8>) -> Response {
    let Some(kind) = image_type(&bytes) else {
        return not_found("No image for this product");
    };
    (
        [
            (header::CONTENT_TYPE, kind),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        bytes,
    )
        .into_response()
}

/// GET /api/card-image/{id_product}
/// Card image from Scryfall by Cardmarket product ID (cached).
async fn card_image_handler(
    State(state): State<AppState>,
    Extension(ClientIp(client)): Extension<ClientIp>,
    id: Result<Path<u64>, PathRejection>,
) -> Response {
    let Ok(Path(id)) = id else {
        return bad_request("Invalid product id");
    };
    if let Some(bytes) = state.image_cache.get_image(id) {
        return image_response(bytes);
    }
    if let Some(refusal) = scryfall_refusal(&state, client, id) {
        return refusal;
    }
    match fetch_image_cached(&state.image_cache, &state.scryfall, id).await {
        Ok(bytes) => image_response(bytes),
        Err(e) => lookup_failed(id, &e),
    }
}

/// GET /api/card-info/{id_product}
/// Cached Scryfall metadata (set name, type, mana cost, rarity, oracle text, purchase links)
async fn card_info_handler(
    State(state): State<AppState>,
    Extension(ClientIp(client)): Extension<ClientIp>,
    id: Result<Path<u64>, PathRejection>,
) -> Response {
    let Ok(Path(id)) = id else {
        return bad_request("Invalid product id");
    };
    let info = match state.image_cache.get_meta(id) {
        Some(info) => info,
        None => {
            if let Some(refusal) = scryfall_refusal(&state, client, id) {
                return refusal;
            }
            match fetch_card_info_cached(&state.image_cache, &state.scryfall, id).await {
                Ok(info) => info,
                Err(e) => return lookup_failed(id, &e),
            }
        }
    };
    // Remember the expansion's name (cached Scryfall data of a known product).
    if let Some(ref set_name) = info.set_name {
        let conn = lock(&state.db);
        if let Ok(Some(id_expansion)) = get_id_expansion_for_product(&conn, id) {
            let _ = upsert_expansion_name(&conn, id_expansion, set_name);
        }
    }
    Json(ApiResponse::ok(info)).into_response()
}

/// POST /api/latest-prices
/// Returns the most recent price row for each requested product ID.
async fn latest_prices_handler(
    State(state): State<AppState>,
    body: Result<Json<BulkPriceRequest>, JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(body)) => body,
        Err(rejection) => return body_rejected(rejection),
    };
    if body.ids.len() > MAX_BULK_IDS {
        return Json(ApiResponse::<()>::err(format!(
            "Too many IDs (max {MAX_BULK_IDS})"
        )))
        .into_response();
    }
    let conn = lock(&state.db);
    match get_latest_prices_bulk(&conn, &body.ids) {
        Ok(prices) => Json(ApiResponse::ok(prices)).into_response(),
        Err(e) => {
            log::error!("Bulk price lookup error: {e}");
            internal()
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
    body: Result<Json<PriceSnapshotRequest>, JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(body)) => body,
        Err(rejection) => return body_rejected(rejection),
    };
    if body.ids.len() > MAX_BULK_IDS {
        return Json(ApiResponse::<()>::err(format!(
            "Too many IDs (max {MAX_BULK_IDS})"
        )))
        .into_response();
    }
    if body.dates.len() > MAX_SNAPSHOT_DATES {
        return Json(ApiResponse::<()>::err(format!(
            "Too many dates (max {MAX_SNAPSHOT_DATES})"
        )))
        .into_response();
    }
    // Dates reach parameterized SQL as-is; validate the shape anyway so
    // malformed input fails loudly instead of silently matching nothing.
    if body
        .dates
        .iter()
        .any(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err())
    {
        return Json(ApiResponse::<()>::err("Invalid date (expected YYYY-MM-DD)")).into_response();
    }
    let conn = lock(&state.db);
    match get_price_snapshots_bulk(&conn, &body.ids, &body.dates) {
        Ok(snapshots) => Json(ApiResponse::ok(snapshots)).into_response(),
        Err(e) => {
            log::error!("Price snapshot lookup error: {e}");
            internal()
        }
    }
}

// ── Router and server ──────────────────────────────────────────────────────

/// The web server's routes with all protections.
pub fn create_router(
    db: Arc<Mutex<Connection>>,
    image_cache: Arc<ImageCache>,
    options: WebOptions,
) -> Router {
    let state = AppState {
        db,
        image_cache,
        scryfall: options.scryfall,
        scryfall_lookups: options.scryfall_lookups,
    };
    let limits = Arc::new(ClientLimits {
        proxies: options.trusted_proxies,
        requests: options.requests,
    });
    let mut router = Router::new()
        .route("/", get(index_handler))
        .route("/api/health", get(health_handler))
        .route("/api/search", get(search_handler))
        .route("/api/prices/{id}", get(prices_handler))
        .route("/api/latest-prices", post(latest_prices_handler))
        .route("/api/price-snapshots", post(price_snapshots_handler))
        .route("/api/card-image/{id}", get(card_image_handler))
        .route("/api/card-info/{id}", get(card_info_handler))
        .with_state(state)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .layer(middleware::from_fn_with_state(limits, limit_requests));
    if let Some(hosts) = options.allowed_hosts {
        router = router.layer(middleware::from_fn_with_state(Arc::new(hosts), check_host));
    }
    router = router.layer(CatchPanicLayer::custom(panic_response));
    for (name, value) in SECURITY_HEADERS {
        router = router.layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        ));
    }
    router.layer(SetResponseHeaderLayer::if_not_present(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    ))
}

/// Starts the web server; listens on `config.bind` (loopback by default).
pub async fn serve(
    db: Arc<Mutex<Connection>>,
    db_path: &std::path::Path,
    config: WebConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create image cache in the same directory as the database
    let db_dir = db_path.parent().ok_or("Failed to get database directory")?;
    let image_cache = Arc::new(ImageCache::new(db_dir));
    let addr = SocketAddr::new(config.bind, config.port);
    if config.bind.is_loopback() {
        log::info!("Web UI listening on {addr} (this machine only)");
    } else {
        log::warn!("Web UI listening on {addr}: reachable from other machines, keep it LAN-only");
    }
    if config.options.allowed_hosts.is_none() {
        log::warn!(
            "No --allowed-host: the Host header is not checked (DNS rebinding); set it when the tracker is reachable by name"
        );
    }
    let app = create_router(db, image_cache, config.options);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
#[path = "web_tests.rs"]
mod tests;
