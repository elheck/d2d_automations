//! Tests for web: every protection of the open LAN API.

use super::*;
use crate::init_schema;
use axum::http::{HeaderMap, Request as HttpRequest};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const JPEG: [u8; 6] = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];

/// A database with products 1, 2 and 3 (named "Card 1" …).
fn database(products: u64) -> (Arc<Mutex<Connection>>, TempDir) {
    let dir = TempDir::new().unwrap();
    let conn = Connection::open(dir.path().join("test.db")).unwrap();
    init_schema(&conn).unwrap();
    for id in 1..=products {
        conn.execute(
            "INSERT INTO products (id_product, name, id_category, category_name, id_expansion,
                 id_metacard, date_added) VALUES (?1, ?2, 1, 'Magic Single', 10, 1, '2026-01-01')",
            rusqlite::params![id, format!("Card {id}")],
        )
        .unwrap();
    }
    (Arc::new(Mutex::new(conn)), dir)
}

struct App {
    router: Router,
    db: Arc<Mutex<Connection>>,
    _dir: TempDir,
}

fn app_with(products: u64, options: WebOptions) -> App {
    let (db, dir) = database(products);
    let cache = Arc::new(ImageCache::new(dir.path()));
    App {
        router: create_router(db.clone(), cache, options),
        db,
        _dir: dir,
    }
}

/// Options with a fake Scryfall (images under `<fake>/img/`) and no spacing.
fn with_scryfall(server: &MockServer) -> WebOptions {
    WebOptions {
        scryfall: Arc::new(Scryfall::with_endpoints(
            &server.uri(),
            &format!("{}/img/", server.uri()),
            Duration::ZERO,
        )),
        ..WebOptions::default()
    }
}

fn request(method: &str, uri: &str, from: [u8; 4], body: Option<String>) -> HttpRequest<Body> {
    let mut builder = HttpRequest::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "localhost:3000");
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let mut req = builder
        .body(body.map_or_else(Body::empty, Body::from))
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((from, 50_000))));
    req
}

async fn send(app: &App, req: HttpRequest<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = app.router.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, headers, body)
}

async fn get(app: &App, uri: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
    send(app, request("GET", uri, [192, 168, 1, 10], None)).await
}

fn json(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap_or(Value::Null)
}

fn card_json(server: &MockServer, image: &str) -> Value {
    serde_json::json!({
        "id": "abc", "name": "Card", "set": "tst", "set_name": "Test Set",
        "collector_number": "1", "rarity": "common",
        "image_uris": { "normal": image.replace("{server}", &server.uri()) }
    })
}

async fn scryfall_card(server: &MockServer, id: u64, image: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/cards/cardmarket/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json(server, image)))
        .mount(server)
        .await;
}

async fn requests_to(server: &MockServer, prefix: &str) -> usize {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.url.path().starts_with(prefix))
        .count()
}

#[tokio::test]
async fn every_response_carries_the_security_headers() {
    let app = app_with(1, WebOptions::default());
    for uri in ["/", "/api/health", "/api/nope", "/api/prices/abc"] {
        let (status, headers, _) = get(&app, uri).await;
        for (name, value) in SECURITY_HEADERS {
            assert_eq!(headers[name], value, "{name} on {uri} ({status})");
        }
        assert_eq!(headers[header::CACHE_CONTROL], "no-store", "{uri}");
    }
    let (_, _, page) = get(&app, "/").await;
    let page = String::from_utf8(page).unwrap();
    // Chart.js comes with an integrity hash the CSP host list relies on.
    assert!(
        page.contains("chart.umd.min.js\" integrity=\"sha384-"),
        "no SRI for Chart.js"
    );
}

#[tokio::test]
async fn searches_are_capped() {
    let app = app_with(MAX_SEARCH_LIMIT as u64 + 20, WebOptions::default());
    let (status, _, body) = get(&app, "/api/search?q=Card&limit=100000").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json(&body)["data"].as_array().unwrap().len(),
        MAX_SEARCH_LIMIT
    );
    let (_, _, body) = get(&app, "/api/search?q=Card&limit=0").await;
    assert_eq!(json(&body)["data"].as_array().unwrap().len(), 1);
    let (_, _, body) = get(&app, "/api/search?q=Card").await;
    assert_eq!(
        json(&body)["data"].as_array().unwrap().len(),
        100,
        "default limit"
    );

    let long = "x".repeat(MAX_SEARCH_QUERY_CHARS + 1);
    let (status, _, body) = get(&app, &format!("/api/search?q={long}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let message = json(&body)["error"].as_str().unwrap().to_string();
    assert!(!message.contains("xxx"), "{message}");
    let ok = "x".repeat(MAX_SEARCH_QUERY_CHARS);
    assert_eq!(
        get(&app, &format!("/api/search?q={ok}")).await.0,
        StatusCode::OK
    );
    let (status, _, body) = get(&app, "/api/search?q=a&limit=lots").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["error"], "Invalid search parameters");
}

#[tokio::test]
async fn price_requests_are_checked() {
    let app = app_with(1, WebOptions::default());
    assert_eq!(get(&app, "/api/prices/1").await.0, StatusCode::OK);
    assert_eq!(get(&app, "/api/prices/1?days=30").await.0, StatusCode::OK);
    let (status, _, body) = get(&app, "/api/prices/abc").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!String::from_utf8_lossy(&body).contains("abc"));
    let (status, _, body) = get(&app, "/api/prices/1?since=yesterday").await;
    assert_eq!(
        (status, json(&body)["error"].as_str()),
        (
            StatusCode::BAD_REQUEST,
            Some("Invalid date (expected YYYY-MM-DD)")
        )
    );
    assert_eq!(get(&app, "/api/prices/999").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bulk_bodies_are_checked_without_echoing_them() {
    let app = app_with(1, WebOptions::default());
    let post = |body: String| request("POST", "/api/latest-prices", [192, 168, 1, 10], Some(body));
    let (status, _, body) = send(&app, post("{\"ids\": [\"secret\"]}".into())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["error"], "Invalid request body");
    let (status, _, _) = send(
        &app,
        post(format!("{{\"ids\": [{}1]}}", "1,".repeat(BODY_LIMIT / 2))),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let ids: Vec<u64> = (0..=MAX_BULK_IDS as u64).collect();
    let (status, _, body) = send(&app, post(serde_json::json!({ "ids": ids }).to_string())).await;
    assert_eq!(
        (status, &json(&body)["success"]),
        (StatusCode::OK, &Value::Bool(false))
    );
    let (status, _, body) = send(&app, post("{\"ids\": [1]}".into())).await;
    assert_eq!(
        (status, &json(&body)["success"]),
        (StatusCode::OK, &Value::Bool(true))
    );
    let snapshot = request(
        "POST",
        "/api/price-snapshots",
        [192, 168, 1, 10],
        Some("{\"ids\": [1], \"dates\": [\"soon\"]}".into()),
    );
    let (_, _, body) = send(&app, snapshot).await;
    assert_eq!(json(&body)["error"], "Invalid date (expected YYYY-MM-DD)");
}

#[tokio::test]
async fn scryfall_is_only_asked_about_known_products() {
    let server = MockServer::start().await;
    let app = app_with(1, with_scryfall(&server));
    for uri in ["/api/card-image/999", "/api/card-info/999"] {
        let (status, _, body) = get(&app, uri).await;
        assert_eq!(
            (status, json(&body)["error"].as_str()),
            (StatusCode::NOT_FOUND, Some("Unknown product"))
        );
    }
    assert_eq!(
        get(&app, "/api/card-image/x").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(requests_to(&server, "/").await, 0, "Scryfall was asked");
}

#[tokio::test]
async fn images_are_fetched_once_and_served_from_the_cache() {
    let server = MockServer::start().await;
    scryfall_card(&server, 1, "{server}/img/1.jpg").await;
    Mock::given(method("GET"))
        .and(path("/img/1.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(JPEG.to_vec()))
        .mount(&server)
        .await;
    let app = app_with(1, with_scryfall(&server));
    for _ in 0..2 {
        let (status, headers, body) = get(&app, "/api/card-image/1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
        assert_eq!(headers[header::CACHE_CONTROL], "public, max-age=86400");
        assert_eq!(body, JPEG);
    }
    let (status, _, body) = get(&app, "/api/card-info/1").await;
    assert_eq!(
        (status, json(&body)["data"]["set_name"].as_str()),
        (StatusCode::OK, Some("Test Set"))
    );
    assert_eq!(
        requests_to(&server, "/cards/").await,
        1,
        "one Scryfall lookup"
    );
    assert_eq!(requests_to(&server, "/img/").await, 1);
    // The expansion name was remembered for the product's expansion.
    let name: String = lock(&app.db)
        .query_row(
            "SELECT name FROM expansion_names WHERE id_expansion = 10",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(name, "Test Set");
}

#[tokio::test]
async fn misses_are_remembered_but_outages_are_not() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cards/cardmarket/1"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "object": "error", "code": "not_found", "status": 404, "details": "No card"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/cards/cardmarket/2"))
        .respond_with(ResponseTemplate::new(503).set_body_string("<html>down</html>"))
        .mount(&server)
        .await;
    let app = app_with(2, with_scryfall(&server));
    for _ in 0..2 {
        let (status, _, body) = get(&app, "/api/card-image/1").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            json(&body)["error"],
            "Scryfall has nothing for this product"
        );
    }
    assert_eq!(
        requests_to(&server, "/cards/cardmarket/1").await,
        1,
        "the miss was remembered"
    );
    for _ in 0..2 {
        let (status, _, body) = get(&app, "/api/card-info/2").await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(json(&body)["error"], "Scryfall cannot be reached right now");
    }
    assert_eq!(
        requests_to(&server, "/cards/cardmarket/2").await,
        2,
        "an outage is asked again"
    );
}

#[tokio::test]
async fn images_only_come_from_the_image_host() {
    let server = MockServer::start().await;
    scryfall_card(&server, 1, "{server}/elsewhere/1.jpg").await;
    let app = app_with(1, with_scryfall(&server));
    let (status, _, _) = get(&app, "/api/card-image/1").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(requests_to(&server, "/elsewhere/").await, 0);
}

#[tokio::test]
async fn files_that_are_not_images_are_refused() {
    let server = MockServer::start().await;
    scryfall_card(&server, 1, "{server}/img/1.jpg").await;
    Mock::given(method("GET"))
        .and(path("/img/1.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<script>alert(1)</script>"))
        .mount(&server)
        .await;
    let app = app_with(1, with_scryfall(&server));
    assert_eq!(
        get(&app, "/api/card-image/1").await.0,
        StatusCode::BAD_GATEWAY
    );
}

#[tokio::test]
async fn scryfall_lookups_have_a_budget_per_client() {
    let server = MockServer::start().await;
    scryfall_card(&server, 1, "{server}/img/1.jpg").await;
    scryfall_card(&server, 2, "{server}/img/2.jpg").await;
    let app = app_with(
        2,
        WebOptions {
            scryfall_lookups: Arc::new(RateLimiter::new(1, 1)),
            ..with_scryfall(&server)
        },
    );
    assert_eq!(get(&app, "/api/card-info/1").await.0, StatusCode::OK);
    // Cached answers cost nothing; a new lookup is over the budget.
    assert_eq!(get(&app, "/api/card-info/1").await.0, StatusCode::OK);
    let (status, headers, _) = get(&app, "/api/card-info/2").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(headers.contains_key(header::RETRY_AFTER));
    let other = send(
        &app,
        request("GET", "/api/card-info/2", [192, 168, 1, 11], None),
    )
    .await;
    assert_eq!(other.0, StatusCode::OK, "another client has its own budget");
}

#[tokio::test]
async fn requests_are_limited_per_client() {
    let app = app_with(
        1,
        WebOptions {
            requests: Arc::new(RateLimiter::new(2, 1)),
            ..WebOptions::default()
        },
    );
    assert_eq!(get(&app, "/api/health").await.0, StatusCode::OK);
    assert_eq!(get(&app, "/api/health").await.0, StatusCode::OK);
    let (status, headers, body) = get(&app, "/api/health").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(headers[header::RETRY_AFTER], "60");
    assert!(json(&body)["error"]
        .as_str()
        .unwrap()
        .starts_with("Too many requests"));
    assert_eq!(headers["x-content-type-options"], "nosniff");
    let other = send(&app, request("GET", "/api/health", [192, 168, 1, 11], None)).await;
    assert_eq!(other.0, StatusCode::OK);
}

#[tokio::test]
async fn forwarded_addresses_only_count_from_trusted_proxies() {
    let app = app_with(
        1,
        WebOptions {
            requests: Arc::new(RateLimiter::new(1, 1)),
            trusted_proxies: TrustedProxies::parse(&["10.0.0.1".to_string()]).unwrap(),
            ..WebOptions::default()
        },
    );
    let via = |peer: [u8; 4], forwarded: &str| {
        let mut req = request("GET", "/api/health", peer, None);
        req.headers_mut()
            .insert("x-forwarded-for", HeaderValue::from_str(forwarded).unwrap());
        req
    };
    // Behind the trusted proxy every client has its own limit.
    assert_eq!(
        send(&app, via([10, 0, 0, 1], "192.168.1.20")).await.0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, via([10, 0, 0, 1], "192.168.1.21")).await.0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, via([10, 0, 0, 1], "192.168.1.20")).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    // Anyone else cannot escape the limit with a made-up header.
    assert_eq!(
        send(&app, via([192, 168, 1, 30], "1.1.1.1")).await.0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, via([192, 168, 1, 30], "2.2.2.2")).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn the_host_allowlist_is_enforced_when_set() {
    let open = app_with(1, WebOptions::default());
    let checked = app_with(
        1,
        WebOptions {
            allowed_hosts: Some(HostAllowlist::new(&["cardscanner.local".to_string()]).unwrap()),
            ..WebOptions::default()
        },
    );
    let to = |host: &str| {
        let mut req = request("GET", "/api/health", [192, 168, 1, 10], None);
        req.headers_mut()
            .insert(header::HOST, HeaderValue::from_str(host).unwrap());
        req
    };
    assert_eq!(
        send(&open, to("evil.example")).await.0,
        StatusCode::OK,
        "opt-in"
    );
    let (status, headers, body) = send(&checked, to("evil.example")).await;
    assert_eq!(
        (status, json(&body)["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("Unknown host name"))
    );
    assert_eq!(headers["x-frame-options"], "DENY");
    assert_eq!(
        send(&checked, to("cardscanner.local:3000")).await.0,
        StatusCode::OK
    );
    assert_eq!(send(&checked, to("localhost:3000")).await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_poisoned_database_lock_does_not_take_the_api_down() {
    let app = app_with(1, WebOptions::default());
    let db = app.db.clone();
    let _ = std::thread::spawn(move || {
        let _guard = db.lock().unwrap();
        panic!("poison the lock");
    })
    .join();
    assert!(app.db.is_poisoned());
    let (status, _, body) = get(&app, "/api/search?q=Card").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["data"][0]["name"], "Card 1");
}

#[test]
fn test_search_params_default_limit() {
    assert_eq!(default_limit(), 100);
}

#[test]
fn test_api_response_serialization() {
    let response: ApiResponse<Vec<i32>> = ApiResponse::ok(vec![1, 2, 3]);
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"success\":true"));
    assert!(json.contains("\"data\":[1,2,3]"));
}

#[test]
fn test_api_response_error_serialization() {
    let response: ApiResponse<()> = ApiResponse::err("Test error");
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"success\":false"));
    assert!(json.contains("\"error\":\"Test error\""));
    assert!(!json.contains("\"data\""));
}
