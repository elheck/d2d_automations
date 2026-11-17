use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use crate::bins::tests::{keep_file, sample_csv};
use axum::http::{header, Method, StatusCode};
use axum::Router;
use check_stock_core::category::DEFAULT_CATEGORY;
use check_stock_core::inventory_db::sync_inventory_on_conn;
use check_stock_core::io::parse_csv;
use serde_json::{json, Value};

/// The sample stock in the database and as the kept import file.
async fn stocked() -> TestApp {
    let app = TestApp::new();
    app.state
        .db
        .write(|conn| {
            let cards = parse_csv(sample_csv().as_bytes(), 100).unwrap();
            sync_inventory_on_conn(conn, &cards, DEFAULT_CATEGORY, "2026-11-14")?;
            Ok(keep_file(conn, &sample_csv()))
        })
        .await
        .unwrap();
    app
}

async fn user(app: &TestApp, name: &str, keys: &[&str]) -> (Router, String) {
    app.add_user(name, keys).await;
    let router = app.router();
    let cookie = login_ok(&router, name, PASSWORD).await;
    (router, cookie)
}

async fn get(
    router: &Router,
    cookie: &str,
    path: &str,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    send(router, request(Method::GET, path, None, Some(cookie))).await
}

async fn search(router: &Router, cookie: &str, query: &str) -> Value {
    let (status, _, body) = get(router, cookie, &format!("/api/search?{query}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

#[tokio::test]
async fn searching_names_sets_and_locations() {
    let app = stocked().await;
    let (router, cookie) = user(&app, "searcher", &["search.use"]).await;
    let all = search(&router, &cookie, "fields=name,set&per_page=25").await;
    assert_eq!(all["total"], 4);
    assert_eq!(all["copies"], 56);
    assert_eq!(all["source"]["file_name"], "inventory-report-Magic.csv");
    let keeper = search(&router, &cookie, "q=keep&fields=name&per_page=25").await;
    assert_eq!(keeper["total"], 2, "two piles of the Keeper");
    let row = &keeper["rows"][0];
    assert_eq!(row["name"], "Keeper");
    assert_eq!(row["location"], "A-0-0-1-L1-R");
    assert_eq!(row["quantity"], 50);
    assert_eq!(row["price"], 1.0);
    assert_eq!(row["product_id"], 1);
    assert_eq!(row["id"].as_str().unwrap().len(), 16);
    let by_bin = search(&router, &cookie, "q=a-0-0-3&fields=location&per_page=25").await;
    assert_eq!(by_bin["total"], 2);
    let none = search(&router, &cookie, "q=a-0-0-3&fields=name&per_page=25").await;
    assert_eq!(none["total"], 0);
    let paged = search(&router, &cookie, "fields=name&per_page=25&page=1").await;
    assert_eq!(paged["rows"], json!([]));
    assert_eq!(paged["total"], 4);

    for bad in [
        "/api/search?fields=name&per_page=7",
        "/api/search?fields=password&per_page=25",
        "/api/search?fields=name&per_page=25&evil=1",
    ] {
        let (status, _, _) = get(&router, &cookie, bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    let long = format!("/api/search?fields=name&per_page=25&q={}", "x".repeat(201));
    let (status, _, _) = get(&router, &cookie, &long).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn without_an_import_the_search_is_empty() {
    let app = TestApp::new();
    let (router, cookie) = user(&app, "searcher", &["search.use"]).await;
    let body = search(&router, &cookie, "fields=name&per_page=25").await;
    assert_eq!(body["source"], Value::Null);
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn price_history_only_for_cards_in_stock() {
    let app = stocked().await;
    let (router, cookie) = user(&app, "searcher", &["search.use"]).await;
    let (status, _, body) = get(&router, &cookie, "/api/search/history/999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_in_stock");
    // No tracker configured in tests.
    let (status, _, body) = get(&router, &cookie, "/api/search/history/2").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"]["code"], "tracker_unavailable");
}

#[tokio::test]
async fn price_history_comes_from_the_tracker() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let tracker = MockServer::start().await;
    let empty: Vec<Option<f64>> = Vec::new();
    let indicators = json!({
        "ema_7": empty, "ema_30": empty, "sma_20": empty, "rsi": empty, "macd": empty,
        "macd_signal": empty, "macd_histogram": empty, "bb_upper": empty, "bb_middle": empty,
        "bb_lower": empty, "roc_7": empty, "roc_30": empty, "bb_percent_b": empty,
        "bb_width": empty,
    });
    let signals = json!({ "momentum_1_7": empty, "momentum_7_30": empty, "floor_ratio": empty });
    let point = |date: &str, trend: f64, foil: f64| {
        json!({ "price_date": date, "avg": null, "low": null, "trend": trend, "avg1": null,
                "avg7": null, "avg30": null, "avg_foil": null, "low_foil": null,
                "trend_foil": foil, "avg1_foil": null, "avg7_foil": null, "avg30_foil": null })
    };
    Mock::given(method("GET"))
        .and(path("/api/prices/2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": {
                "product": { "id_product": 2, "name": "Spark", "category_name": "Magic Single",
                             "id_expansion": 1, "expansion_name": "Test Set" },
                "history": [point("2026-11-01", 1.0, 5.0), point("2026-11-15", 1.5, 4.0)],
                "indicators": indicators,
                "cardmarket_signals": signals,
            }
        })))
        .mount(&tracker)
        .await;
    let app = stocked()
        .await
        .with(|state, _| state.with_inventory_sync(Some(&tracker.uri())));
    let (router, cookie) = user(&app, "searcher", &["search.use"]).await;
    let (status, _, body) = get(&router, &cookie, "/api/search/history/2").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "Spark");
    assert_eq!(body["points"].as_array().unwrap().len(), 2);
    assert_eq!(body["trend"], 1.5);
    assert!((body["change_7d"].as_f64().unwrap() - 50.0).abs() < 1e-9);
    let (_, _, foil) = get(&router, &cookie, "/api/search/history/2?foil=true").await;
    assert_eq!(foil["trend"], 4.0);
}

async fn discard(router: &Router, cookie: &str, items: Value) -> (StatusCode, Value) {
    let (status, _, body) = send(
        router,
        request(
            Method::POST,
            "/api/inventory/discard",
            Some(json!({ "items": items })),
            Some(cookie),
        ),
    )
    .await;
    (status, body)
}

#[tokio::test]
async fn discarding_from_search() {
    let app = stocked().await;
    let (router, cookie) = user(&app, "clerk", &["search.use", "inventory.discard"]).await;
    let found = search(&router, &cookie, "q=spark&fields=name&per_page=25").await;
    let spark = found["rows"][0]["id"].clone();
    let (_, _, info) = get(&router, &cookie, "/api/data-info").await;
    let before = info["inventory_version"].as_i64().unwrap();

    let (status, body) = discard(&router, &cookie, json!([{ "pile": spark, "quantity": 2 }])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["copies"], 2);
    assert_eq!(
        body["value"],
        Value::Null,
        "no money without financials.view"
    );
    assert_eq!(body["data_version"], before + 1);
    assert!(app.state.market.is_outdated());
    let after = search(&router, &cookie, "q=spark&fields=name&per_page=25").await;
    assert_eq!(after["rows"][0]["quantity"], 1);

    let (status, body) = discard(&router, &cookie, json!([{ "pile": spark, "quantity": 2 }])).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "not_enough");
    assert_eq!(body["error"]["message"], "Only 1 copy of \"Spark\" is left");

    let (status, _, history) = get(&router, &cookie, "/api/inventory/discards").await;
    assert_eq!(status, StatusCode::OK);
    let id = history[0]["id"].as_i64().unwrap();
    let (status, headers, csv) = get(
        &router,
        &cookie,
        &format!("/api/inventory/discards/{id}/csv"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        format!("attachment; filename=\"discarded_cards_{id}.csv\"")
    );
    assert!(csv
        .as_str()
        .unwrap()
        .lines()
        .nth(1)
        .unwrap()
        .starts_with("2,-2,Spark,"));
    let (status, _, _) = get(&router, &cookie, "/api/inventory/discards/999/csv").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let actions = app.audit_actions().await;
    assert_eq!(actions[0].0, "inventory.discarded");
}

#[tokio::test]
async fn discard_requests_are_checked() {
    let app = stocked().await;
    let (router, cookie) = user(
        &app,
        "owner",
        &["search.use", "inventory.discard", "financials.view"],
    )
    .await;
    let found = search(&router, &cookie, "q=ember&fields=name&per_page=25").await;
    let ember = found["rows"][0]["id"].clone();
    for (items, code) in [
        (json!([]), "invalid_discard"),
        (json!([{ "pile": ember, "quantity": 0 }]), "invalid_discard"),
        (
            json!([{ "pile": "0123456789abcdef", "quantity": 1 }]),
            "stock_changed",
        ),
        (
            json!([{ "pile": "x".repeat(30), "quantity": 1 }]),
            "invalid_discard",
        ),
    ] {
        let (status, body) = discard(&router, &cookie, items).await;
        assert!(status.is_client_error(), "{body}");
        assert_eq!(body["error"]["code"], code);
    }
    let (status, _, body) = send(
        &router,
        request(
            Method::POST,
            "/api/inventory/discard",
            Some(json!({ "items": [{ "pile": ember, "quantity": 1, "price": 0 }] })),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the browser sends no card data: {body}"
    );
    let (status, body) = discard(&router, &cookie, json!([{ "pile": ember, "quantity": 2 }])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["value"], 2.0, "listed value for the owner");
}

#[tokio::test]
async fn searchers_without_the_discard_permission_cannot_discard() {
    let app = stocked().await;
    let (router, cookie) = user(&app, "searcher", &["search.use"]).await;
    let (status, _) = discard(&router, &cookie, json!([{ "pile": "x", "quantity": 1 }])).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_discard_makes_bin_move_lists_outdated() {
    let app = stocked().await;
    let (router, cookie) = user(
        &app,
        "clerk",
        &[
            "search.use",
            "inventory.discard",
            "analysis.view",
            "bins.use",
        ],
    )
    .await;
    let (status, _, created) = send(
        &router,
        request(
            Method::POST,
            "/api/bins/list",
            Some(json!({ "kind": "consolidation" })),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let found = search(&router, &cookie, "q=spark&fields=name&per_page=25").await;
    let (status, _) = discard(
        &router,
        &cookie,
        json!([{ "pile": found["rows"][0]["id"], "quantity": 1 }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, _, list) = get(&router, &cookie, "/api/bins/list").await;
    assert_eq!(
        list["outdated"], true,
        "planned on the stock before the discard"
    );
    let (status, _, again) = send(
        &router,
        request(
            Method::POST,
            "/api/bins/list",
            Some(json!({ "kind": "consolidation" })),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["list"]["outdated"], false);
    let spark = again["list"]["moves"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "Spark")
        .unwrap();
    assert_eq!(spark["quantity"], 2, "the plan sees the discard");
}
