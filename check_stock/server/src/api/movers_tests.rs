use super::*;
use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use crate::market::tests::{fake_tracker, stock};
use crate::market::MarketService;
use axum::http::Method;
use axum::Router;
use check_stock_core::api::inventory_sync::InventorySyncClient;
use serde_json::{json, Value};
use std::time::Duration;

const QUERY: &str = "field=trend&min_price=0.3&min_age=0&direction=all&sort=change7&desc=true";

async fn analyst(app: &TestApp) -> (Router, String) {
    app.add_user("analyst", &["analysis.view"]).await;
    let router = app.router();
    let cookie = login_ok(&router, "analyst", PASSWORD).await;
    (router, cookie)
}

async fn get(router: &Router, uri: &str, cookie: &str) -> (StatusCode, Value) {
    let (status, _, body) = send(router, request(Method::GET, uri, None, Some(cookie))).await;
    (status, body)
}

async fn load(
    router: &Router,
    cookie: &str,
    refresh: bool,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    send(
        router,
        request(
            Method::POST,
            "/api/movers/load",
            Some(json!({ "refresh": refresh })),
            Some(cookie),
        ),
    )
    .await
}

/// A test app with the fake market and three cards in stock, loaded.
async fn loaded() -> (TestApp, Router, String, wiremock::MockServer) {
    let tracker = fake_tracker().await;
    let url = tracker.uri();
    let app = TestApp::new().with(|state, _| {
        state.with_market(
            MarketService::new(Some(InventorySyncClient::new(&url))).with_cooldown(Duration::ZERO),
        )
    });
    stock(&app.state.db).await;
    let (router, cookie) = analyst(&app).await;
    let (status, _, body) = load(&router, &cookie, false).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let market = app.state.market.clone();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while market.loaded().is_none() {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    (app, router, cookie, tracker)
}

#[tokio::test]
async fn the_status_follows_the_load() {
    let (_app, router, cookie, _tracker) = loaded().await;
    let (status, body) = get(&router, "/api/movers/status", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["configured"], true);
    assert_eq!(body["job"]["state"], "finished");
    assert_eq!(body["loaded"]["cards"], 3);
    assert_eq!(body["loaded"]["products"], 3);
    assert_eq!(body["loaded"]["version"], 1);
    // Already loaded today: asking again starts nothing.
    let (status, _, body) = load(&router, &cookie, false).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["loaded"]["version"], 1);
    assert_eq!(body["job"]["state"], "finished");
}

#[tokio::test]
async fn movers_show_how_prices_moved() {
    let (_app, router, cookie, _tracker) = loaded().await;
    let (status, body) = get(&router, &format!("/api/movers?{QUERY}"), &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 3);
    let rows = body["rows"].as_array().unwrap();
    // Sorted by the 7-day change, highest first.
    assert_eq!(rows[0]["name"], "Riser");
    assert!(rows[0]["pct_7d"].as_f64().unwrap() > 0.0);
    assert_eq!(rows[0]["market"], 10.0);
    assert_eq!(rows[0]["listed"], 9.0);
    assert_eq!(rows[0]["quantity"], 2);
    assert!(rows[0]["age_days"].as_i64().unwrap() > 300);
    assert_eq!(rows[0]["location"], "A-1-1-1-L2-R");
    assert!(rows[0]["cardmarket_url"]
        .as_str()
        .unwrap()
        .ends_with("idProduct=101"));
    assert_eq!(rows[2]["name"], "Faller");
    assert!(rows[2]["pct_7d"].as_f64().unwrap() < 0.0);

    let (_, body) = get(
        &router,
        "/api/movers?field=trend&min_price=0&min_age=0&direction=fallers&sort=name&desc=false",
        &cookie,
    )
    .await;
    let names: Vec<&str> = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Faller"]);
    let (_, body) = get(
        &router,
        "/api/movers?field=trend&min_price=6&min_age=0&direction=all&sort=name&desc=false",
        &cookie,
    )
    .await;
    let names: Vec<&str> = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Faller", "Riser"], "the flat card is under 6 €");
    let (_, body) = get(
        &router,
        "/api/movers?field=trend&min_price=0&min_age=36500&direction=all&sort=name&desc=false",
        &cookie,
    )
    .await;
    assert_eq!(body["error"]["code"], "invalid_params");
}

#[tokio::test]
async fn movers_need_loaded_data_and_valid_parameters() {
    let app = TestApp::new();
    let (router, cookie) = analyst(&app).await;
    let (status, body) = get(&router, &format!("/api/movers?{QUERY}"), &cookie).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "not_loaded");
    let (status, _, body) = load(&router, &cookie, false).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "not_configured");
    for (query, code) in [
        (
            "field=trend&min_price=-1&min_age=0&direction=all&sort=change7&desc=true",
            "invalid_params",
        ),
        (
            "field=trend&min_price=1001&min_age=0&direction=all&sort=change7&desc=true",
            "invalid_params",
        ),
        (
            "field=trend&min_price=NaN&min_age=0&direction=all&sort=change7&desc=true",
            "invalid_params",
        ),
        (
            "field=trend&min_price=1&min_age=-1&direction=all&sort=change7&desc=true",
            "invalid_params",
        ),
        (
            "field=trend&min_price=1&min_age=0&direction=up&sort=change7&desc=true",
            "invalid_request",
        ),
        (
            "field=trend&min_price=1&min_age=0&direction=all&sort=change7&desc=true&x=1",
            "invalid_request",
        ),
    ] {
        let (status, body) = get(&router, &format!("/api/movers?{query}"), &cookie).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_eq!(body["error"]["code"], code, "{query}");
    }
}

#[tokio::test]
async fn a_refresh_right_after_a_load_waits() {
    let tracker = fake_tracker().await;
    let url = tracker.uri();
    let app = TestApp::new().with(|state, _| {
        state.with_market(
            MarketService::new(Some(InventorySyncClient::new(&url)))
                .with_cooldown(Duration::from_secs(60)),
        )
    });
    stock(&app.state.db).await;
    let (router, cookie) = analyst(&app).await;
    load(&router, &cookie, false).await;
    let market = app.state.market.clone();
    while market.loaded().is_none() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (status, headers, body) = load(&router, &cookie, true).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "too_soon");
    assert!(headers.contains_key(header::RETRY_AFTER));
}
