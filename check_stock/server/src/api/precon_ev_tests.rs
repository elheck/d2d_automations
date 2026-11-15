use super::*;
use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use crate::precon::tests::{wait_for, FakeServices, ARMY, FAMILY};
use crate::precon::PreconService;
use axum::http::Method;
use axum::Router;
use check_stock_core::api::inventory_sync::InventorySyncClient;
use check_stock_core::api::mtgjson::MtgjsonClient;
use serde_json::{json, Value};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PARAMS: &str = "field=trend&sealed_field=trend&cutoff=0.5&foil_needs_sales=true";

async fn user(app: &TestApp) -> (Router, String) {
    app.add_user("analyst", &["precon_ev.use"]).await;
    let router = app.router();
    let cookie = login_ok(&router, "analyst", PASSWORD).await;
    (router, cookie)
}

/// A logged-in user with `precon_ev.use`, against the fake services.
async fn with_fakes(fake: &FakeServices) -> (TestApp, Router, String) {
    let tracker = fake.tracker.uri();
    let mtgjson = fake.mtgjson.uri();
    let app = TestApp::new().with(|state, dir| {
        state.with_precon(
            PreconService::new(
                &dir.join("cache"),
                Some(InventorySyncClient::new(&tracker)),
                MtgjsonClient::new(&mtgjson),
            )
            .with_cooldown(Duration::ZERO),
        )
    });
    let (router, cookie) = user(&app).await;
    (app, router, cookie)
}

async fn get(router: &Router, uri: &str, cookie: &str) -> (StatusCode, Value) {
    let (status, _, body) = send(router, request(Method::GET, uri, None, Some(cookie))).await;
    (status, body)
}

async fn start_load(router: &Router, cookie: &str) -> (StatusCode, axum::http::HeaderMap, Value) {
    send(
        router,
        request(
            Method::POST,
            "/api/precon-ev/load",
            Some(json!({ "refresh_index": false })),
            Some(cookie),
        ),
    )
    .await
}

/// Loads and waits until the sales history is in.
async fn loaded(app: &TestApp, router: &Router, cookie: &str) {
    let (status, _, body) = start_load(router, cookie).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let precon = app.state.precon.clone();
    wait_for(|| precon.loaded().is_some_and(|l| l.data.sales.is_some())).await;
}

#[tokio::test]
async fn the_status_says_what_is_loaded() {
    let fake = FakeServices::start().await;
    let (app, router, cookie) = with_fakes(&fake).await;
    let (status, body) = get(&router, "/api/precon-ev/status", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["configured"], true);
    assert_eq!(body["load"]["state"], "idle");
    assert_eq!(body["loaded"], Value::Null);

    loaded(&app, &router, &cookie).await;
    let (_, body) = get(&router, "/api/precon-ev/status", &cookie).await;
    assert_eq!(body["load"]["state"], "finished");
    assert_eq!(body["sales"]["state"], "finished");
    let info = &body["loaded"];
    assert_eq!(info["products"], 2);
    assert_eq!(info["with_decklist"], 2);
    assert_eq!(info["deck_list_version"], "v2");
    assert_eq!(info["sales_loaded"], true);
    assert_eq!(info["index_stale"], false);
    assert!(info["version"].as_u64().unwrap() >= 2);
}

#[tokio::test]
async fn the_ranking_follows_the_parameters() {
    let fake = FakeServices::start().await;
    let (app, router, cookie) = with_fakes(&fake).await;
    let (status, body) = get(
        &router,
        &format!("/api/precon-ev/ranking?{PARAMS}&sort=ratio&desc=true"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "not_loaded");

    loaded(&app, &router, &cookie).await;
    let (status, body) = get(
        &router,
        &format!("/api/precon-ev/ranking?{PARAMS}&sort=ratio&desc=true"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 2);
    assert_eq!(body["ready"], 2);
    let rows = body["rows"].as_array().unwrap();
    // Army: EV 10 (the 0.20 card is below the cutoff) / 40 = 0.25;
    // Family: 5 / 30 ≈ 0.17.
    assert_eq!(rows[0]["id_product"], ARMY);
    assert_eq!(
        rows[0]["name"],
        "Commander: Bloomburrow: \"Animated Army\" Commander Deck"
    );
    assert_eq!(rows[0]["state"], "ready");
    assert_eq!(rows[0]["ev"]["ev"], 10.0);
    assert_eq!(rows[0]["ev"]["gross"], 10.2);
    assert_eq!(rows[0]["ratio"], 0.25);
    assert_eq!(rows[0]["sealed_price"], 40.0);
    assert_eq!(rows[0]["set_label"], "Bloomburrow Commander");
    assert_eq!(rows[0]["release_date"], "2024-08-02");
    assert!(rows[0]["ev"]["liquid"].is_number(), "sales history applied");
    assert!(rows[0]["cardmarket_url"]
        .as_str()
        .unwrap()
        .ends_with("idProduct=774134"));
    assert_eq!(rows[1]["id_product"], FAMILY);

    // Lower cutoff: the 0.20 card counts too.
    let (_, body) = get(
        &router,
        "/api/precon-ev/ranking?field=trend&sealed_field=trend&cutoff=0.1&foil_needs_sales=true&sort=ev&desc=true",
        &cookie,
    )
    .await;
    assert_eq!(body["rows"][0]["ev"]["ev"], 10.2);
    // Sorting and filter.
    let (_, body) = get(
        &router,
        &format!("/api/precon-ev/ranking?{PARAMS}&sort=name&desc=false&filter=family"),
        &cookie,
    )
    .await;
    let rows = body["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id_product"], FAMILY);
    assert_eq!(body["total"], 2);
}

#[tokio::test]
async fn invalid_parameters_are_refused() {
    let fake = FakeServices::start().await;
    let (app, router, cookie) = with_fakes(&fake).await;
    loaded(&app, &router, &cookie).await;
    for (query, code) in [
        ("field=trend&sealed_field=avg7&cutoff=0.5&foil_needs_sales=true&sort=ratio&desc=true", "invalid_params"),
        ("field=trend&sealed_field=trend&cutoff=101&foil_needs_sales=true&sort=ratio&desc=true", "invalid_params"),
        ("field=trend&sealed_field=trend&cutoff=-1&foil_needs_sales=true&sort=ratio&desc=true", "invalid_params"),
        ("field=trend&sealed_field=trend&cutoff=NaN&foil_needs_sales=true&sort=ratio&desc=true", "invalid_params"),
        ("field=median&sealed_field=trend&cutoff=0.5&foil_needs_sales=true&sort=ratio&desc=true", "invalid_request"),
        ("field=trend&sealed_field=trend&cutoff=0.5&foil_needs_sales=true&sort=ratio&desc=true&x=1", "invalid_request"),
        ("field=trend&sealed_field=trend&cutoff=0.5&foil_needs_sales=true", "invalid_request"),
    ] {
        let (status, body) = get(&router, &format!("/api/precon-ev/ranking?{query}"), &cookie).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_eq!(body["error"]["code"], code, "{query}");
    }
    let long = "x".repeat(MAX_FILTER_LEN + 1);
    let (status, _) = get(
        &router,
        &format!("/api/precon-ev/ranking?{PARAMS}&sort=ratio&desc=true&filter={long}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_product_shows_its_cards() {
    let fake = FakeServices::start().await;
    let (app, router, cookie) = with_fakes(&fake).await;
    loaded(&app, &router, &cookie).await;
    let (status, body) = get(
        &router,
        &format!("/api/precon-ev/products/{ARMY}?{PARAMS}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["row"]["id_product"], ARMY);
    let decks = body["decks"].as_array().unwrap();
    assert_eq!(decks.len(), 1);
    assert_eq!(decks[0]["name"], "Animated Army");
    assert_eq!(decks[0]["ev"], 10.0);
    let cards = decks[0]["cards"].as_array().unwrap();
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0]["rank"], 1);
    assert_eq!(cards[0]["unit"], 10.0);
    assert_eq!(cards[0]["counted"], true);
    assert_eq!(cards[0]["top5"], true);
    assert_eq!(cards[0]["board"], "main");
    assert_eq!(cards[0]["note"], "");
    assert!(cards[0]["sale_days"].as_u64().unwrap() > 0);
    assert!(cards[0]["cardmarket_url"]
        .as_str()
        .unwrap()
        .ends_with("idProduct=101"));
    assert_eq!(cards[1]["counted"], false);
    assert_eq!(cards[1]["note"], "below cutoff");

    let (status, body) = get(
        &router,
        &format!("/api/precon-ev/products/123?{PARAMS}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "unknown_product");
}

#[tokio::test]
async fn a_product_history_has_weekly_points() {
    let fake = FakeServices::start().await;
    let (app, router, cookie) = with_fakes(&fake).await;
    loaded(&app, &router, &cookie).await;
    let (status, body) = get(
        &router,
        &format!("/api/precon-ev/products/{ARMY}/history?{PARAMS}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let points = body["points"].as_array().unwrap();
    assert!(points.len() > 10, "a year of weeks");
    let last = points.last().unwrap();
    assert_eq!(last["ev"], 10.0);
    assert_eq!(last["sealed"], 40.0);
    assert_eq!(last["coverage"], 1.0);
    let (status, body) = get(
        &router,
        &format!("/api/precon-ev/products/123/history?{PARAMS}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "no_decklist");
}

#[tokio::test]
async fn loads_are_not_started_too_often() {
    let fake = FakeServices::start().await;
    let tracker = fake.tracker.uri();
    let mtgjson = fake.mtgjson.uri();
    let app = TestApp::new().with(|state, dir| {
        state.with_precon(
            PreconService::new(
                &dir.join("cache"),
                Some(InventorySyncClient::new(&tracker)),
                MtgjsonClient::new(&mtgjson),
            )
            .with_cooldown(Duration::from_secs(60)),
        )
    });
    let (router, cookie) = user(&app).await;
    let (status, _, _) = start_load(&router, &cookie).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    // While it runs, asking again just answers with the status.
    let (status, _, body) = start_load(&router, &cookie).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(body["load"]["state"] == "running" || body["load"]["state"] == "finished");
    let precon = app.state.precon.clone();
    wait_for(|| !precon.load_status().is_running()).await;
    let (status, headers, body) = start_load(&router, &cookie).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "too_soon");
    let wait: u64 = headers[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=60).contains(&wait));
}

#[tokio::test]
async fn without_a_tracker_the_page_says_so() {
    let app = TestApp::new();
    let (router, cookie) = user(&app).await;
    let (_, body) = get(&router, "/api/precon-ev/status", &cookie).await;
    assert_eq!(body["configured"], false);
    let (status, _, body) = start_load(&router, &cookie).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "not_configured");
    let (status, body) = get(
        &router,
        &format!("/api/precon-ev/products/{ARMY}/history?{PARAMS}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "not_configured");
}

#[tokio::test]
async fn failed_decklists_and_tracker_errors_show_no_details() {
    let fake = FakeServices {
        tracker: MockServer::start().await,
        mtgjson: MockServer::start().await,
    };
    fake.mount_tracker().await;
    // Family Matters fails to download.
    Mock::given(method("GET"))
        .and(path("/decks/FamilyMatters_BLC.json"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal secret"))
        .mount(&fake.mtgjson)
        .await;
    fake.mount_mtgjson().await;
    let (app, router, cookie) = with_fakes(&fake).await;
    loaded(&app, &router, &cookie).await;
    let (_, body) = get(
        &router,
        &format!("/api/precon-ev/ranking?{PARAMS}&sort=name&desc=false"),
        &cookie,
    )
    .await;
    let family = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id_product"] == FAMILY)
        .unwrap()
        .clone();
    assert_eq!(family["state"], "deck_unavailable");
    assert_eq!(family["note"], "decklist download failed");
    assert_eq!(family["note_detail"], Value::Null);
    assert!(!body.to_string().contains("127.0.0.1"));
    assert!(!body.to_string().contains("secret"));
}

#[tokio::test]
async fn settings_are_kept_per_user() {
    let app = TestApp::new();
    let (router, cookie) = user(&app).await;
    let (status, body) = get(&router, "/api/precon-ev/settings", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["settings"], body["defaults"]);
    assert_eq!(body["defaults"]["field"], "trend");
    assert_eq!(body["defaults"]["cutoff"], 0.5);
    assert_eq!(body["defaults"]["sort"], "ratio");

    let mut mine = body["defaults"].clone();
    mine["field"] = json!("avg7");
    mine["sealed_field"] = json!("low");
    mine["sort"] = json!("liquid");
    mine["hide_no_decklist"] = json!(true);
    let (status, _, saved) = send(
        &router,
        request(
            Method::PUT,
            "/api/precon-ev/settings",
            Some(mine.clone()),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (_, body) = get(&router, "/api/precon-ev/settings", &cookie).await;
    assert_eq!(body["settings"], mine);
    // The same settings again: nothing written, no second audit entry.
    let (status, _, _) = send(
        &router,
        request(
            Method::PUT,
            "/api/precon-ev/settings",
            Some(mine.clone()),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let mut bad = mine.clone();
    bad["sealed_field"] = json!("avg1");
    let (status, _, body) = send(
        &router,
        request(
            Method::PUT,
            "/api/precon-ev/settings",
            Some(bad),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_params");
    let entries = app
        .state
        .db
        .read(|conn| crate::audit::list(conn, None, 50))
        .await
        .unwrap();
    let changes = entries
        .iter()
        .filter(|e| e.action == "settings.changed" && e.target.as_deref() == Some("precon_ev"))
        .count();
    assert_eq!(changes, 1);
}
