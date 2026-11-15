use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use crate::bins::tests::{keep_file, sample_csv};
use axum::http::{header, Method, StatusCode};
use axum::Router;
use serde_json::{json, Value};

async fn stocked() -> TestApp {
    let app = TestApp::new();
    app.state
        .db
        .write(|conn| Ok(keep_file(conn, &sample_csv())))
        .await
        .unwrap();
    app
}

async fn checker(app: &TestApp) -> (Router, String) {
    app.add_user("checker", &["stock_checker.use"]).await;
    let router = app.router();
    let cookie = login_ok(&router, "checker", PASSWORD).await;
    (router, cookie)
}

async fn post(
    router: &Router,
    cookie: &str,
    path: &str,
    body: Value,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    send(
        router,
        request(Method::POST, path, Some(body), Some(cookie)),
    )
    .await
}

async fn check(router: &Router, cookie: &str, list: &str) -> (StatusCode, Value) {
    let (status, _, body) = post(
        router,
        cookie,
        "/api/stock-checker/check",
        json!({ "wantslist": list, "language": "English", "language_only": false }),
    )
    .await;
    (status, body)
}

#[tokio::test]
async fn a_wantslist_is_matched_against_the_stock() {
    let app = stocked().await;
    let (router, cookie) = checker(&app).await;
    let (status, body) = check(
        &router,
        &cookie,
        "4 Spark\n60 Keeper\n1 Unknown Card\nnot a card row\n",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["wanted"], 65);
    assert_eq!(body["found"], 54);
    let spark = &body["groups"][0];
    assert_eq!(
        (
            spark["name"].as_str(),
            spark["wanted"].as_i64(),
            spark["found"].as_i64()
        ),
        (Some("Spark"), Some(4), Some(3))
    );
    assert_eq!(spark["piles"][0]["take"], 3);
    assert_eq!(spark["piles"][0]["pile"]["location"], "A-0-0-2-L2-R");
    let keeper = &body["groups"][1];
    assert_eq!(keeper["found"], 51, "both piles");
    assert_eq!(body["groups"][2]["found"], 0);
    assert_eq!(body["unparseable"], json!(["not a card row"]));
}

#[tokio::test]
async fn deck_links_are_not_fetched() {
    let app = stocked().await;
    let (router, cookie) = checker(&app).await;
    for link in [
        "https://archidekt.com/decks/123456/my_deck",
        "4 Spark\nhttps://www.moxfield.com/decks/abc",
    ] {
        let (status, body) = check(&router, &cookie, link).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "deck_link");
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("copy as text"));
    }
}

#[tokio::test]
async fn wantslists_are_limited() {
    let app = stocked().await;
    let (router, cookie) = checker(&app).await;
    let (status, _) = check(&router, &cookie, &"1 Spark\n".repeat(2_001)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = check(&router, &cookie, &"x".repeat(256 * 1024 + 1)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let (status, body) = check(&router, &cookie, "10001 Spark").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_quantity");
    let (status, _, _) = post(
        &router,
        &cookie,
        "/api/stock-checker/check",
        json!({ "wantslist": "1 Spark", "language": "Klingon", "language_only": false }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn outputs_and_the_stock_update_csv_come_from_the_stock() {
    let app = stocked().await;
    let (router, cookie) = checker(&app).await;
    let (_, body) = check(&router, &cookie, "2 Spark\n1 Ember").await;
    let spark = body["groups"][0]["piles"][0]["pile"]["id"].clone();
    let ember = body["groups"][1]["piles"][0]["pile"]["id"].clone();
    let groups = json!([
        { "name": "Spark", "wanted": 2, "items": [{ "pile": spark, "quantity": 2 }] },
        { "name": "Ember", "wanted": 1, "items": [{ "pile": ember, "quantity": 1 }] },
    ]);
    for format in ["picking", "invoice", "summary"] {
        let (status, _, out) = post(
            &router,
            &cookie,
            "/api/stock-checker/output",
            json!({ "format": format, "discount": 10.0, "groups": groups }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{format}: {out}");
        let text = out["text"].as_str().unwrap();
        assert!(
            text.contains("Spark") && text.contains("Ember"),
            "{format}: {text}"
        );
    }
    let (status, headers, csv) = post(
        &router,
        &cookie,
        "/api/stock-checker/update-csv",
        json!({ "items": [{ "pile": spark, "quantity": 2 }] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"stock_update.csv\""
    );
    let line = csv.as_str().unwrap().lines().nth(1).unwrap().to_string();
    assert!(line.starts_with("2,-2,Spark,"), "{line}");

    // More copies than the pile has, or an unknown pile: refused.
    for (items, code) in [
        (json!([{ "pile": spark, "quantity": 4 }]), "not_enough"),
        (
            json!([{ "pile": "0123456789abcdef", "quantity": 1 }]),
            "stock_changed",
        ),
    ] {
        let (status, _, body) = post(
            &router,
            &cookie,
            "/api/stock-checker/update-csv",
            json!({ "items": items }),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], code);
    }
    // Card data from the browser is not accepted.
    let (status, _, _) = post(
        &router,
        &cookie,
        "/api/stock-checker/update-csv",
        json!({ "items": [{ "pile": spark, "quantity": 1, "price": "0.01" }] }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _) = post(
        &router,
        &cookie,
        "/api/stock-checker/output",
        json!({ "format": "summary", "discount": 120.0, "groups": groups }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn without_an_import_there_is_nothing_to_check() {
    let app = TestApp::new();
    let (router, cookie) = checker(&app).await;
    let (status, body) = check(&router, &cookie, "1 Spark").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "no_import");
}
