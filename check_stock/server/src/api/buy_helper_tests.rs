use super::*;
use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use crate::api::{CSRF_HEADER, CSRF_VALUE};
use axum::body::Body;
use axum::http::{Method, Request as HttpRequest};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const HEADER: &str =
    "cardmarketId,quantity,name,set,setCode,cn,condition,language,isFoil,price,location,rarity";

/// A card export with `(name, rarity, price, quantity)` rows.
fn csv(rows: &[(&str, &str, &str, &str)]) -> String {
    let mut out = format!("{HEADER}\n");
    for (i, (name, rarity, price, qty)) in rows.iter().enumerate() {
        out.push_str(&format!(
            "{},{qty},\"{}\",Alpha,LEA,{},NM,English,,{price},,{rarity}\n",
            1000 + i,
            name.replace('"', "\"\""),
            i + 1
        ));
    }
    out
}

fn sample() -> String {
    csv(&[
        ("Black Lotus", "rare", "15000", "1"),
        ("Lightning Bolt", "common", "0.25", "4"),
        ("Counterspell", "common", "0.75", "2"),
        ("Llanowar Elves", "common", "0.10", "1000"),
    ])
}

fn default_params() -> Value {
    serde_json::to_value(defaults()).unwrap()
}

async fn buyer() -> (TestApp, Router, String) {
    let app = TestApp::new();
    app.add_user("buyer", &["buy_helper.use"]).await;
    let router = app.router();
    let cookie = login_ok(&router, "buyer", PASSWORD).await;
    (app, router, cookie)
}

async fn offer(router: &Router, cookie: &str, body: Value) -> (StatusCode, Value) {
    let (status, _, body) = send(
        router,
        request(
            Method::POST,
            "/api/buy-helper/offer",
            Some(body),
            Some(cookie),
        ),
    )
    .await;
    (status, body)
}

/// A POST with a raw body of `size` bytes (for the size limits).
fn raw_post(path: &str, body: Vec<u8>, cookie: &str) -> HttpRequest<Body> {
    HttpRequest::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::HOST, "localhost:8080")
        .header(CSRF_HEADER, CSRF_VALUE)
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

fn close(a: &Value, b: f64) -> bool {
    (a.as_f64().unwrap() - b).abs() < 1e-9
}

#[tokio::test]
async fn an_offer_splits_singles_and_bulk() {
    let (_app, router, cookie) = buyer().await;
    let (status, body) = offer(
        &router,
        &cookie,
        json!({ "csv": sample(), "params": default_params() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["rows"], 4);
    let s = &body["summary"];
    // Singles: the rare, and the common worth at least 0.50.
    assert_eq!(s["single_rows"], 2);
    assert_eq!(s["single_cards"], 3);
    assert!(close(&s["single_market_value"], 15001.5));
    assert!(close(&s["single_offer"], 9000.9));
    // Bulk: 1004 cards at 10 € per 1000.
    assert_eq!(s["bulk_rows"], 2);
    assert_eq!(s["bulk_cards"], 1004);
    assert!(close(&s["bulk_market_value"], 101.0));
    assert!(close(&s["bulk_offer"], 10.04));
    assert!(close(&s["total_offer"], 9010.94));
    // Most valuable first.
    assert_eq!(body["singles_total"], 2);
    assert_eq!(body["singles"][0]["name"], "Black Lotus");
    assert_eq!(body["singles"][0]["set_code"], "LEA");
    assert!(close(&body["singles"][0]["offer"], 9000.0));
    assert_eq!(body["singles"][1]["name"], "Counterspell");
    assert_eq!(body["singles"][1]["quantity"], 2);
}

#[tokio::test]
async fn the_settings_change_the_offer() {
    let (_app, router, cookie) = buyer().await;
    let params = json!({
        "single_common": true, "single_uncommon": false, "single_rare": true, "single_mythic": true,
        "min_single_price": null, "single_buy_percent": 50.0, "bulk_rate": 0.0, "bulk_batch": 1000
    });
    let (status, body) = offer(
        &router,
        &cookie,
        json!({ "csv": sample(), "params": params }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["summary"]["single_rows"], 4);
    assert_eq!(body["summary"]["bulk_rows"], 0);
    assert!(close(
        &body["summary"]["total_offer"],
        (15000.0 + 1.0 + 1.5 + 100.0) / 2.0
    ));
}

#[tokio::test]
async fn only_the_most_valuable_singles_are_listed() {
    let (_app, router, cookie) = buyer().await;
    let names: Vec<String> = (0..PREVIEW_LIMIT + 5)
        .map(|i| format!("Card {i:03}"))
        .collect();
    let prices: Vec<String> = (0..PREVIEW_LIMIT + 5)
        .map(|i| format!("{}.00", i + 1))
        .collect();
    let rows: Vec<(&str, &str, &str, &str)> = names
        .iter()
        .zip(&prices)
        .map(|(n, p)| (n.as_str(), "rare", p.as_str(), "1"))
        .collect();
    let (_, body) = offer(
        &router,
        &cookie,
        json!({ "csv": csv(&rows), "params": default_params() }),
    )
    .await;
    assert_eq!(body["singles_total"], PREVIEW_LIMIT + 5);
    assert_eq!(body["singles"].as_array().unwrap().len(), PREVIEW_LIMIT);
    assert_eq!(
        body["singles"][0]["name"],
        format!("Card {:03}", PREVIEW_LIMIT + 4)
    );
}

#[tokio::test]
async fn the_export_is_a_csv_download_without_formulas() {
    let (_app, router, cookie) = buyer().await;
    let list = csv(&[
        (
            "=HYPERLINK(\"http://evil.example\",\"click\")",
            "rare",
            "5",
            "1",
        ),
        ("+2 Mace", "common", "0.10", "3"),
    ]);
    let response = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/buy-helper/export",
            Some(json!({ "csv": list, "params": default_params() })),
            Some(&cookie),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    assert_eq!(headers[header::CONTENT_TYPE], "text/csv; charset=utf-8");
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"buy_offer.csv\""
    );
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.starts_with("name,set,setCode"));
    assert!(text.contains("'=HYPERLINK"));
    assert!(text.contains("'+2 Mace"));
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    for record in reader.records() {
        for cell in record.unwrap().iter() {
            assert!(!cell.starts_with(['=', '+', '@']), "formula cell {cell:?}");
        }
    }
}

#[tokio::test]
async fn invalid_settings_are_refused() {
    let (_app, router, cookie) = buyer().await;
    let base = default_params();
    for (field, value) in [
        ("single_buy_percent", json!(100.5)),
        ("single_buy_percent", json!(-1)),
        ("bulk_rate", json!(-0.01)),
        ("bulk_rate", json!(1e7)),
        ("bulk_batch", json!(0)),
        ("bulk_batch", json!(1_000_001)),
        ("min_single_price", json!(-1)),
        ("min_single_price", json!(1e9)),
    ] {
        let mut params = base.clone();
        params[field] = value.clone();
        let (status, body) = offer(
            &router,
            &cookie,
            json!({ "csv": sample(), "params": params }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{field}={value}");
        assert_eq!(body["error"]["code"], "invalid_params", "{field}={value}");
    }
    // Unknown or missing fields: the usual request error (as on every route).
    let mut extra = base.clone();
    extra["sneaky"] = json!(true);
    for body in [
        json!({ "csv": sample(), "params": extra }),
        json!({ "csv": sample() }),
        json!({ "csv": sample(), "params": base, "more": 1 }),
    ] {
        let (status, body) = offer(&router, &cookie, body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "invalid_request");
    }
}

#[tokio::test]
async fn unreadable_files_are_explained_without_echoing_them() {
    let (_app, router, cookie) = buyer().await;
    for (list, expect) in [
        ("secret,stuff\nmy-password,1234\n".to_string(), "line 2"),
        (format!("{HEADER}\n1,2,secret-row\n"), "line 2"),
    ] {
        let (status, body) = offer(
            &router,
            &cookie,
            json!({ "csv": list, "params": default_params() }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["code"], "invalid_csv");
        let message = body["error"]["message"].as_str().unwrap();
        assert!(message.contains(expect), "{message}");
        assert!(
            !message.contains("secret") && !message.contains("password"),
            "{message}"
        );
    }
    // An empty file is simply an empty offer.
    let (status, body) = offer(
        &router,
        &cookie,
        json!({ "csv": "", "params": default_params() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rows"], 0);
}

#[tokio::test]
async fn implausible_values_are_refused() {
    let (_app, router, cookie) = buyer().await;
    let long_name = "x".repeat(500);
    for rows in [
        vec![("Too Many", "rare", "1", "1000001")],
        vec![("Too Dear", "rare", "1000000.01", "1")],
        vec![("Negative", "rare", "-1", "1")],
        vec![(long_name.as_str(), "rare", "-5", "1")],
    ] {
        let (status, body) = offer(
            &router,
            &cookie,
            json!({ "csv": csv(&rows), "params": default_params() }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{rows:?}");
        assert_eq!(body["error"]["code"], "invalid_csv");
        assert!(body["error"]["message"].as_str().unwrap().len() < 300);
    }
    // NaN and infinity count as no price (0), like an unreadable price.
    let (status, body) = offer(
        &router,
        &cookie,
        json!({ "csv": csv(&[("Odd", "rare", "NaN", "1"), ("Odder", "rare", "inf", "2")]), "params": default_params() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(close(&body["summary"]["total_offer"], 0.0));
}

#[tokio::test]
async fn too_many_rows_are_refused() {
    let (_app, router, cookie) = buyer().await;
    let mut list = format!("{HEADER}\n");
    for i in 0..=MAX_ROWS {
        list.push_str(&format!("{i},1,C,,,,NM,EN,,1,,common\n"));
    }
    let (status, body) = offer(
        &router,
        &cookie,
        json!({ "csv": list, "params": default_params() }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_csv");
    assert!(body["error"]["message"].as_str().unwrap().contains("50000"));
}

#[tokio::test]
async fn uploads_may_be_large_but_not_too_large() {
    let (_app, router, cookie) = buyer().await;
    // ~1 MB: far above the usual 64 KiB body limit, accepted here.
    let mut rows = Vec::new();
    let name = "N".repeat(200);
    for _ in 0..4000 {
        rows.push((name.as_str(), "common", "0.10", "1"));
    }
    let list = csv(&rows);
    assert!(list.len() > 800_000);
    let (status, body) = offer(
        &router,
        &cookie,
        json!({ "csv": list, "params": default_params() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["rows"], 4000);

    // CSV text above the limit (body still below the route's body limit).
    let big = "a".repeat(MAX_CSV_BYTES + 1);
    let (status, body) = offer(
        &router,
        &cookie,
        json!({ "csv": big, "params": default_params() }),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "too_large");
    assert!(body["error"]["message"].as_str().unwrap().contains("6 MB"));

    // A body above the route's limit.
    let body = json!({ "csv": "a".repeat(BODY_LIMIT), "params": default_params() }).to_string();
    let (status, _, answer) = send(
        &router,
        raw_post("/api/buy-helper/export", body.into_bytes(), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer["error"]["code"], "too_large");
}

#[tokio::test]
async fn other_routes_keep_the_small_body_limit() {
    let (_app, router, cookie) = buyer().await;
    let body =
        json!({ "username": "a".repeat(super::super::BODY_LIMIT), "password": "x" }).to_string();
    let (status, _, answer) = send(
        &router,
        raw_post("/api/auth/login", body.into_bytes(), &cookie),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer["error"]["code"], "too_large");
    let settings = json!({ "single_common": "a".repeat(super::super::BODY_LIMIT) }).to_string();
    let request = HttpRequest::builder()
        .method(Method::PUT)
        .uri("/api/buy-helper/settings")
        .header(header::HOST, "localhost:8080")
        .header(CSRF_HEADER, CSRF_VALUE)
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(settings))
        .unwrap();
    let (status, _, _) = send(&router, request).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn anonymous_uploads_are_refused_before_reading_them() {
    let (_app, router, _) = buyer().await;
    let body = json!({ "csv": "a".repeat(2_000_000), "params": default_params() }).to_string();
    let mut request = raw_post("/api/buy-helper/offer", body.into_bytes(), "x=y");
    request.headers_mut().remove(header::COOKIE);
    let (status, _, answer) = send(&router, request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer["error"]["code"], "unauthenticated");
}

// ── Settings ──────────────────────────────────────────────────────────────

async fn get_settings(router: &Router, cookie: &str) -> Value {
    let (status, _, body) = send(
        router,
        request(Method::GET, "/api/buy-helper/settings", None, Some(cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn put_settings(router: &Router, cookie: &str, params: Value) -> (StatusCode, Value) {
    let (status, _, body) = send(
        router,
        request(
            Method::PUT,
            "/api/buy-helper/settings",
            Some(params),
            Some(cookie),
        ),
    )
    .await;
    (status, body)
}

fn changed_entries(app_actions: &[(String, String, Value)]) -> usize {
    app_actions
        .iter()
        .filter(|(action, target, _)| action == "settings.changed" && target == "buy_helper")
        .count()
}

async fn audit_actions(app: &TestApp) -> Vec<(String, String, Value)> {
    app.state
        .db
        .read(|conn| crate::audit::list(conn, None, 200))
        .await
        .unwrap()
        .into_iter()
        .map(|e| (e.action, e.target.unwrap_or_default(), e.details))
        .collect()
}

#[tokio::test]
async fn settings_start_with_the_defaults_and_are_kept_per_user() {
    let (app, router, cookie) = buyer().await;
    let first = get_settings(&router, &cookie).await;
    assert_eq!(first["params"], default_params());
    assert_eq!(first["defaults"], default_params());

    let mut mine = default_params();
    mine["single_buy_percent"] = json!(55.0);
    mine["min_single_price"] = json!(null);
    let (status, body) = put_settings(&router, &cookie, mine.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["params"], mine);
    assert_eq!(get_settings(&router, &cookie).await["params"], mine);
    assert_eq!(changed_entries(&audit_actions(&app).await), 1);

    // The same values again: nothing written, no second audit entry.
    put_settings(&router, &cookie, mine.clone()).await;
    assert_eq!(changed_entries(&audit_actions(&app).await), 1);

    // Another user has their own.
    app.add_user("other", &["buy_helper.use"]).await;
    let other = login_ok(&router, "other", PASSWORD).await;
    assert_eq!(
        get_settings(&router, &other).await["params"],
        default_params()
    );
}

#[tokio::test]
async fn invalid_settings_are_not_stored() {
    let (app, router, cookie) = buyer().await;
    let mut bad = default_params();
    bad["single_buy_percent"] = json!(250);
    let (status, body) = put_settings(&router, &cookie, bad).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_params");
    let mut unknown = default_params();
    unknown["extra"] = json!(1);
    let (status, _) = put_settings(&router, &cookie, unknown).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        get_settings(&router, &cookie).await["params"],
        default_params()
    );
    assert_eq!(changed_entries(&audit_actions(&app).await), 0);
}

#[tokio::test]
async fn stored_settings_that_no_longer_fit_fall_back_to_the_defaults() {
    let (app, router, cookie) = buyer().await;
    // Incomplete (e.g. from an older version), and complete but out of range.
    let mut out_of_range = default_params();
    out_of_range["single_buy_percent"] = json!(900.0);
    for stored in [json!({ "single_buy_percent": 50 }), out_of_range] {
        let text = stored.to_string();
        app.state
            .db
            .write(move |conn| {
                conn.execute(
                    "INSERT INTO srv_user_settings (user_id, key, value, updated_at)
                     SELECT id, 'buy_helper', ?1, 'then' FROM srv_users WHERE username = 'buyer'
                     ON CONFLICT (user_id, key) DO UPDATE SET value = excluded.value",
                    [text],
                )
            })
            .await
            .unwrap();
        assert_eq!(
            get_settings(&router, &cookie).await["params"],
            default_params(),
            "{stored}"
        );
    }
}
