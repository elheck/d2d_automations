use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use crate::bins::tests::{keep_file, row, sample_csv};
use axum::http::{Method, StatusCode};
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

async fn picker(app: &TestApp, name: &str) -> (Router, String) {
    app.add_user(name, &["stock_checker.use"]).await;
    let router = app.router();
    let cookie = login_ok(&router, name, PASSWORD).await;
    (router, cookie)
}

async fn call(
    router: &Router,
    cookie: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (status, _, body) = send(router, request(method, path, body, Some(cookie))).await;
    (status, body)
}

async fn piles(router: &Router, cookie: &str) -> (Value, Value) {
    let (_, body) = call(
        router,
        cookie,
        Method::POST,
        "/api/stock-checker/check",
        Some(json!({ "wantslist": "2 Spark\n1 Ember", "language": "English", "language_only": false })),
    )
    .await;
    (
        body["groups"][0]["piles"][0]["pile"]["id"].clone(),
        body["groups"][1]["piles"][0]["pile"]["id"].clone(),
    )
}

#[tokio::test]
async fn a_picking_list_is_made_ticked_and_shared_between_devices() {
    let app = stocked().await;
    let (router, cookie) = picker(&app, "picker").await;
    let (spark, ember) = piles(&router, &cookie).await;
    let (status, list) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/picking",
        Some(json!({ "items": [{ "pile": ember, "quantity": 1 }, { "pile": spark, "quantity": 2 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["outdated"], false);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items[0]["location"], "A-0-0-2-L2-R", "shelf order");
    assert_eq!(items[0]["quantity"], 2);
    assert_eq!(items[0]["image"], "/api/picking/image/tst/2");

    // Another device of the same user.
    let other_device = login_ok(&router, "picker", PASSWORD).await;
    let (status, body) = call(
        &router,
        &other_device,
        Method::PUT,
        "/api/picking/picked",
        Some(json!({ "piles": [spark], "picked": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["picked"], 1);
    let (_, list) = call(&router, &cookie, Method::GET, "/api/picking", None).await;
    assert_eq!(list["items"][0]["picked"], true);

    let (status, _) = call(
        &router,
        &cookie,
        Method::PUT,
        "/api/picking/picked",
        Some(json!({ "piles": ["0123456789abcdef"], "picked": true })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Others do not see it.
    let (_, someone) = picker(&app, "someone").await;
    let (_, theirs) = call(&router, &someone, Method::GET, "/api/picking", None).await;
    assert_eq!(theirs, Value::Null);

    let actions = app.audit_actions().await;
    assert!(actions.iter().any(|a| a.0 == "picking.created"));
    let (status, _) = call(&router, &cookie, Method::DELETE, "/api/picking", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, gone) = call(&router, &cookie, Method::GET, "/api/picking", None).await;
    assert_eq!(gone, Value::Null);
}

#[tokio::test]
async fn a_newer_import_marks_the_list_but_keeps_it() {
    let app = stocked().await;
    let (router, cookie) = picker(&app, "picker").await;
    let (spark, _) = piles(&router, &cookie).await;
    call(
        &router,
        &cookie,
        Method::POST,
        "/api/picking",
        Some(json!({ "items": [{ "pile": spark, "quantity": 1 }] })),
    )
    .await;
    app.state
        .db
        .write(|conn| {
            Ok(keep_file(
                conn,
                &(sample_csv() + &row(9, "New", 1, "A-0-0-9") + "\n"),
            ))
        })
        .await
        .unwrap();
    let (_, list) = call(&router, &cookie, Method::GET, "/api/picking", None).await;
    assert_eq!(list["outdated"], true);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    let (status, _) = call(
        &router,
        &cookie,
        Method::PUT,
        "/api/picking/picked",
        Some(json!({ "piles": [spark], "picked": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "picking goes on");
}

#[tokio::test]
async fn picking_lists_need_piles_from_the_stock() {
    let app = stocked().await;
    let (router, cookie) = picker(&app, "picker").await;
    for (items, status) in [
        (json!([]), StatusCode::UNPROCESSABLE_ENTITY),
        (
            json!([{ "pile": "0123456789abcdef", "quantity": 1 }]),
            StatusCode::CONFLICT,
        ),
    ] {
        let (got, body) = call(
            &router,
            &cookie,
            Method::POST,
            "/api/picking",
            Some(json!({ "items": items })),
        )
        .await;
        assert_eq!(got, status, "{body}");
    }
}
