use super::*;
use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use crate::market::tests::{fake_tracker, stock, stock_card, FLAT, RISER};
use crate::market::MarketService;
use axum::http::Method;
use axum::Router;
use check_stock_core::api::inventory_sync::InventorySyncClient;
use check_stock_core::category::DEFAULT_CATEGORY;
use check_stock_core::inventory_db::sync_inventory_on_conn;
use serde_json::{json, Value};
use std::time::Duration;

const QUERY: &str = "field=trend&threshold=5&filter=all&sort=priority&desc=true";

async fn user_with(app: &TestApp, name: &str, keys: &[&str]) -> (Router, String) {
    app.add_user(name, keys).await;
    let router = app.router();
    let cookie = login_ok(&router, name, PASSWORD).await;
    (router, cookie)
}

async fn get(router: &Router, uri: &str, cookie: &str) -> (StatusCode, Value) {
    let (status, _, body) = send(router, request(Method::GET, uri, None, Some(cookie))).await;
    (status, body)
}

/// A test app with the fake market; `stock_it` puts cards in stock.
async fn app_with_market(tracker: &wiremock::MockServer) -> TestApp {
    let url = tracker.uri();
    TestApp::new().with(|state, _| {
        state.with_market(
            MarketService::new(Some(InventorySyncClient::new(&url))).with_cooldown(Duration::ZERO),
        )
    })
}

async fn load_market(router: &Router, cookie: &str, app: &TestApp) {
    let (status, _, body) = send(
        router,
        request(
            Method::POST,
            "/api/mispricing/load",
            Some(json!({ "refresh": false })),
            Some(cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let market = app.state.market.clone();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while market.loaded().is_none() {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn listings_are_judged_against_the_market_without_money_totals() {
    let tracker = fake_tracker().await;
    let app = app_with_market(&tracker).await;
    stock(&app.state.db).await;
    let (router, cookie) = user_with(&app, "pricer", &["mispricing.view"]).await;
    let (status, body) = get(&router, &format!("/api/mispricing?{QUERY}"), &cookie).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "not_loaded");

    load_market(&router, &cookie, &app).await;
    // The status route shows the shared data.
    let (_, status_body) = get(&router, "/api/mispricing/status", &cookie).await;
    assert_eq!(status_body["loaded"]["cards"], 3);

    let (status, body) = get(&router, &format!("/api/mispricing?{QUERY}"), &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let s = &body["summary"];
    // Riser listed 9 vs market 10 (−10 %), Faller 12 vs 10 (+20 %), Flat 5 vs 5.
    assert_eq!(s["underpriced_rows"], 1);
    assert_eq!(s["overpriced_rows"], 1);
    assert_eq!(s["fair_rows"], 1);
    assert_eq!(s["no_data_rows"], 0);
    for money in ["upside", "excess", "listed_value", "market_value"] {
        assert_eq!(s[money], Value::Null, "{money} without financials.view");
    }
    assert_eq!(body["total"], 3);
    let rows = body["rows"].as_array().unwrap();
    let riser = rows.iter().find(|r| r["name"] == "Riser").unwrap();
    assert_eq!(riser["verdict"], "underpriced");
    assert_eq!(riser["listed"], 9.0);
    assert_eq!(riser["market"], 10.0);
    assert!(riser["pct_7d"].as_f64().unwrap() > 0.0, "the market rose");
    assert!(riser["impact"].as_f64().unwrap() > 1.9);
    assert!(riser["cardmarket_url"]
        .as_str()
        .unwrap()
        .ends_with(&format!("idProduct={RISER}")));
    let faller = rows.iter().find(|r| r["name"] == "Faller").unwrap();
    assert_eq!(faller["verdict"], "overpriced");
    assert_eq!(body["conflicts_total"], 0);

    let (_, body) = get(
        &router,
        "/api/mispricing?field=trend&threshold=5&filter=underpriced&sort=name&desc=false",
        &cookie,
    )
    .await;
    let names: Vec<&str> = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Riser"]);
    assert_eq!(
        body["summary"]["overpriced_rows"], 1,
        "the summary covers all listings"
    );
}

#[tokio::test]
async fn money_totals_need_the_financials_permission() {
    let tracker = fake_tracker().await;
    let app = app_with_market(&tracker).await;
    stock(&app.state.db).await;
    let (router, cookie) = user_with(&app, "owner", &["mispricing.view", "financials.view"]).await;
    load_market(&router, &cookie, &app).await;
    let (_, body) = get(&router, &format!("/api/mispricing?{QUERY}"), &cookie).await;
    let s = &body["summary"];
    // Riser: 2 copies × (10 − 9); Faller: 1 copy × (12 − 10).
    assert_eq!(s["upside"], 2.0);
    assert_eq!(s["excess"], 2.0);
    assert_eq!(s["listed_value"], 2.0 * 9.0 + 12.0 + 4.0 * 5.0);
    assert_eq!(s["market_value"], 2.0 * 10.0 + 10.0 + 4.0 * 5.0);
}

#[tokio::test]
async fn contradicting_listings_are_reported() {
    let tracker = fake_tracker().await;
    let app = app_with_market(&tracker).await;
    let mut cheap = stock_card(FLAT, "Flat", "1", "5.00");
    cheap.location = Some("A-1-1-1-L2-R".to_string());
    // A worse condition (EX) priced above the NM copy.
    let mut dear = stock_card(FLAT, "Flat", "1", "7.00");
    dear.condition = "EX".to_string();
    dear.location = Some("A-1-1-2-L3-R".to_string());
    app.state
        .db
        .write(move |conn| {
            sync_inventory_on_conn(conn, &[cheap, dear], DEFAULT_CATEGORY, "2026-01-01").map(|_| ())
        })
        .await
        .unwrap();
    let (router, cookie) = user_with(&app, "pricer", &["mispricing.view"]).await;
    load_market(&router, &cookie, &app).await;
    let (_, body) = get(&router, &format!("/api/mispricing?{QUERY}"), &cookie).await;
    assert_eq!(body["conflicts_total"], 1);
    assert_eq!(body["conflicts"][0]["kind"], "condition_inversion");
    assert_eq!(body["conflicts"][0]["name"], "Flat");
    let details = body["conflicts"][0]["details"].as_str().unwrap();
    assert!(
        details.contains("5.00") && details.contains("7.00"),
        "{details}"
    );
}

#[tokio::test]
async fn invalid_parameters_are_refused() {
    let tracker = fake_tracker().await;
    let app = app_with_market(&tracker).await;
    stock(&app.state.db).await;
    let (router, cookie) = user_with(&app, "pricer", &["mispricing.view"]).await;
    load_market(&router, &cookie, &app).await;
    for (query, code) in [
        (
            "field=trend&threshold=101&filter=all&sort=priority&desc=true",
            "invalid_params",
        ),
        (
            "field=trend&threshold=-1&filter=all&sort=priority&desc=true",
            "invalid_params",
        ),
        (
            "field=trend&threshold=NaN&filter=all&sort=priority&desc=true",
            "invalid_params",
        ),
        (
            "field=trend&threshold=5&filter=cheap&sort=priority&desc=true",
            "invalid_request",
        ),
        (
            "field=trend&threshold=5&filter=all&sort=profit&desc=true",
            "invalid_request",
        ),
        (
            "field=trend&threshold=5&filter=all&sort=priority&desc=true&x=1",
            "invalid_request",
        ),
    ] {
        let (status, body) = get(&router, &format!("/api/mispricing?{query}"), &cookie).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_eq!(body["error"]["code"], code, "{query}");
    }
}
