use super::*;
use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use axum::http::Method;
use axum::Router;
use serde_json::{json, Value};

const MAGIC: &str = "inventory-report-Magic.csv";

/// An export with cards `1..=n`, one copy each.
fn export(n: usize) -> String {
    let mut csv = String::from(
        "cardmarketId,quantity,name,set,setCode,cn,condition,language,isFoil,isSigned,\
         finishType,price,comment,location,nameDE,nameES,nameFR,nameIT,rarity,listedAt\n",
    );
    for i in 1..=n {
        csv.push_str(&format!(
            "{i},1,Card {i},Test Set,TST,{i},NM,English,,,Regular,1.00,,A-0-0-{i}-L1-R,,,,,Common,\n"
        ));
    }
    csv
}

async fn importer(app: &TestApp) -> (Router, String) {
    app.add_user("importer", &["inventory.import"]).await;
    let router = app.router();
    let cookie = login_ok(&router, "importer", PASSWORD).await;
    (router, cookie)
}

async fn post(router: &Router, cookie: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let (status, _, body) = send(
        router,
        request(Method::POST, path, Some(body), Some(cookie)),
    )
    .await;
    (status, body)
}

async fn preview_file(router: &Router, cookie: &str, csv: &str) -> (StatusCode, Value) {
    post(
        router,
        cookie,
        "/api/inventory/preview",
        json!({ "file_name": MAGIC, "csv": csv }),
    )
    .await
}

async fn confirm_file(
    router: &Router,
    cookie: &str,
    csv: &str,
    typed: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({ "file_name": MAGIC, "csv": csv });
    if let Some(typed) = typed {
        body["confirm_sold"] = json!(typed);
    }
    post(router, cookie, "/api/inventory/confirm", body).await
}

async fn import(router: &Router, cookie: &str, csv: &str) -> Value {
    let (status, body) = preview_file(router, cookie, csv).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = confirm_file(router, cookie, csv, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn get(router: &Router, cookie: &str, path: &str) -> (StatusCode, Value) {
    let (status, _, body) = send(router, request(Method::GET, path, None, Some(cookie))).await;
    (status, body)
}

#[tokio::test]
async fn preview_then_confirm_imports_the_file() {
    let app = TestApp::new();
    let (router, cookie) = importer(&app).await;
    import(&router, &cookie, &export(5)).await;

    let csv = export(3);
    let (status, preview) = preview_file(&router, &cookie, &csv).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["category"], "Magic");
    assert_eq!(preview["rows"], 3);
    assert_eq!(preview["copies_in_file"], 3);
    assert_eq!(preview["numbers"]["copies_sold"], 2);
    assert_eq!(preview["numbers"]["copies_before"], 5);
    assert_eq!(preview["numbers"]["copies_after"], 3);
    assert_eq!(preview["blocked"], false);
    assert_eq!(preview["new_category"], false);
    assert_eq!(preview["valid_minutes"], 30);

    let (status, done) = confirm_file(&router, &cookie, &csv, None).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["version"], 2);
    assert_eq!(done["forced"], false);
    assert_eq!(done["zeroed"], 2);
    assert!(app.state.market.is_outdated(), "market data is read again");

    let (_, info) = get(&router, &cookie, "/api/data-info").await;
    assert_eq!(info["inventory_version"], 2);
    let (status, history) = get(&router, &cookie, "/api/inventory/imports").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(history[0]["kind"], "import");
    assert_eq!(history[0]["user_name"], "importer");
    assert_eq!(history[0]["copies_sold"], 2);
    let actions = app.audit_actions().await;
    assert_eq!(actions[0].0, "inventory.imported");
    assert_eq!(actions[0].1, "Magic");
    assert_eq!(actions[0].2["numbers"]["copies_sold"], 2);
}

#[tokio::test]
async fn a_blocked_import_needs_the_typed_number() {
    let app = TestApp::new();
    let (router, cookie) = importer(&app).await;
    import(&router, &cookie, &export(200)).await;
    let cut_off = export(20);
    let (_, preview) = preview_file(&router, &cookie, &cut_off).await;
    assert_eq!(preview["blocked"], true);
    assert_eq!(preview["numbers"]["copies_sold"], 180);

    let (status, body) = confirm_file(&router, &cookie, &cut_off, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "confirmation_needed");
    let (status, body) = confirm_file(&router, &cookie, &cut_off, Some("18")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "confirmation_wrong");
    let (status, body) = confirm_file(&router, &cookie, &cut_off, Some(&"1".repeat(33))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "confirmation_wrong");

    let (status, done) = confirm_file(&router, &cookie, &cut_off, Some("180")).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["forced"], true);
    assert_eq!(app.audit_actions().await[0].0, "inventory.import_forced");
}

#[tokio::test]
async fn confirming_needs_the_checked_file() {
    let app = TestApp::new();
    let (router, cookie) = importer(&app).await;
    let (status, body) = confirm_file(&router, &cookie, &export(3), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "no_preview");

    preview_file(&router, &cookie, &export(3)).await;
    let (status, body) = confirm_file(&router, &cookie, &export(4), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "other_file");

    let (status, _, _) = send(
        &router,
        request(
            Method::DELETE,
            "/api/inventory/pending",
            None,
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = confirm_file(&router, &cookie, &export(3), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "no_preview");
}

#[tokio::test]
async fn another_users_import_makes_a_check_outdated() {
    let app = TestApp::new();
    let (router, cookie) = importer(&app).await;
    app.add_user("second", &["inventory.import"]).await;
    let second = login_ok(&router, "second", PASSWORD).await;
    import(&router, &cookie, &export(5)).await;
    preview_file(&router, &cookie, &export(4)).await;
    import(&router, &second, &export(3)).await;
    let (status, body) = confirm_file(&router, &cookie, &export(4), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "outdated");
}

#[tokio::test]
async fn files_that_are_not_exports_are_refused_without_quoting_them() {
    let app = TestApp::new();
    let (router, cookie) = importer(&app).await;
    let (status, body) = preview_file(&router, &cookie, "secret,stuff\n=cmd,1\n").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_file");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(
        !message.contains("secret") && !message.contains("cmd"),
        "{message}"
    );

    let (status, body) = post(
        &router,
        &cookie,
        "/api/inventory/preview",
        json!({ "file_name": "inventory-report-<x>.csv", "csv": export(1) }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_file");

    let (status, _) = post(
        &router,
        &cookie,
        "/api/inventory/preview",
        json!({ "file_name": MAGIC, "csv": export(1), "extra": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "unknown fields");
}

#[tokio::test]
async fn files_and_bodies_over_the_limits_are_refused() {
    let app = TestApp::new();
    let (router, cookie) = importer(&app).await;
    // Within the body limit, over the file limit.
    let big = "x".repeat(MAX_IMPORT_BYTES + 1);
    let (status, body) = preview_file(&router, &cookie, &big).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "too_large");
    // Over the body limit: refused before it is parsed.
    let huge = "x".repeat(BODY_LIMIT);
    let (status, body) = preview_file(&router, &cookie, &huge).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "too_large");
    // Other routes keep the normal limit.
    let (status, _) = post(
        &router,
        &cookie,
        "/api/auth/password",
        json!({ "current_password": "x".repeat(70 * 1024), "new_password": "y" }),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn the_test_copy_import_never_touches_files_outside_the_data_folder() {
    let app = TestApp::new();
    let (router, cookie) = importer(&app).await;
    import(&router, &cookie, &export(2)).await;
    let made: Vec<String> = std::fs::read_dir(app.dir.path())
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".bak"))
        .collect();
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(made[0].starts_with("inventory.db.import-"));
}
