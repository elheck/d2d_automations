use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use crate::bins::tests::{keep_file, row, sample_csv, HEADER};
use axum::http::{header, Method, StatusCode};
use axum::Router;
use serde_json::{json, Value};

async fn user(app: &TestApp, name: &str, keys: &[&str]) -> (Router, String) {
    app.add_user(name, keys).await;
    let router = app.router();
    let cookie = login_ok(&router, name, PASSWORD).await;
    (router, cookie)
}

async fn mover(app: &TestApp) -> (Router, String) {
    user(app, "mover", &["analysis.view", "bins.use"]).await
}

async fn call(
    router: &Router,
    cookie: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    send(router, request(method, path, body, Some(cookie))).await
}

async fn get(router: &Router, cookie: &str, path: &str) -> (StatusCode, Value) {
    let (status, _, body) = call(router, cookie, Method::GET, path, None).await;
    (status, body)
}

async fn keep(app: &TestApp, csv: String) -> i64 {
    app.state
        .db
        .write(move |conn| Ok(keep_file(conn, &csv)))
        .await
        .unwrap()
}

async fn make_list(router: &Router, cookie: &str, body: Value) -> (StatusCode, Value) {
    let (status, _, body) = call(router, cookie, Method::POST, "/api/bins/list", Some(body)).await;
    (status, body)
}

#[tokio::test]
async fn without_an_import_on_the_server_there_is_nothing_to_analyse() {
    let app = TestApp::new();
    let (router, cookie) = mover(&app).await;
    let (status, body) = get(&router, &cookie, "/api/bins?min_free=0&sort=free").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["source"], Value::Null);
    let (status, body) = get(&router, &cookie, "/api/bins/consolidation?threshold=20").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "no_import");
    let (status, _) = make_list(&router, &cookie, json!({ "kind": "defrag" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, body) = get(&router, &cookie, "/api/bins/list").await;
    assert_eq!((status, body), (StatusCode::OK, Value::Null));
}

#[tokio::test]
async fn the_bin_table_lists_fill_and_free_slots() {
    let app = TestApp::new();
    let version = keep(&app, sample_csv()).await;
    let (router, cookie) = user(&app, "analyst", &["analysis.view"]).await;
    let (status, body) = get(&router, &cookie, "/api/bins?min_free=0&sort=location").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["source"]["version"], version);
    assert_eq!(body["source"]["file_name"], "inventory-report-Magic.csv");
    assert_eq!(body["capacity"], 60);
    assert_eq!(body["bins_total"], 3);
    assert_eq!(body["cards_total"], 56);
    let bins: Vec<(String, i64)> = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["bin"].as_str().unwrap().to_string(),
                r["free"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        bins,
        [
            ("A-0-0-1".to_string(), 10),
            ("A-0-0-2".to_string(), 57),
            ("A-0-0-3".to_string(), 57)
        ]
    );
    let (_, body) = get(&router, &cookie, "/api/bins?min_free=20&sort=free").await;
    let rows = body["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["piles"], 1, "shelf order among equal free slots");

    for bad in [
        "/api/bins?min_free=61&sort=free",
        "/api/bins?min_free=0&sort=random",
    ] {
        let (status, _) = get(&router, &cookie, bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[tokio::test]
async fn the_bin_csv_is_spreadsheet_safe() {
    let app = TestApp::new();
    keep(
        &app,
        format!("{HEADER}\n{}\n", row(1, "Bolt", 1, "=A-0-0-1-L1-R")),
    )
    .await;
    let (router, cookie) = user(&app, "analyst", &["analysis.view"]).await;
    let (status, headers, body) = call(
        &router,
        &cookie,
        Method::GET,
        "/api/bins/export?min_free=0&sort=free",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"bin_analysis.csv\""
    );
    assert_eq!(
        body.as_str().unwrap(),
        "bin,cards,piles,free\n'=A-0-0-1,1,1,59\n"
    );
}

#[tokio::test]
async fn fragmented_variants_and_the_consolidation_summary() {
    let app = TestApp::new();
    keep(&app, sample_csv()).await;
    let (router, cookie) = user(&app, "analyst", &["analysis.view"]).await;
    let (status, body) = get(&router, &cookie, "/api/bins/fragmented").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 1);
    assert_eq!(body["variants"][0]["name"], "Keeper");
    assert_eq!(body["variants"][0]["bins"], 2);
    assert_eq!(body["variants"][0]["piles"][1]["location"], "A-0-0-3-L3-R");
    assert_eq!(body["defrag"]["moves"], 1);

    let (status, body) = get(&router, &cookie, "/api/bins/consolidation?threshold=20").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["bins_freed"], json!(["A-0-0-2", "A-0-0-3"]));
    assert_eq!(body["moves"], 3);
    assert_eq!(body["cards_moved"], 6);
    assert_eq!(body["too_large"], false);
    let (status, body) = get(&router, &cookie, "/api/bins/consolidation?threshold=0").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_threshold");
}

#[tokio::test]
async fn a_move_list_is_ticked_and_exported_with_exact_values() {
    let app = TestApp::new();
    // A comment that looks like a formula must stay as it is: Cardmarket
    // imports the file again.
    let csv = sample_csv().replace(
        &row(2, "Spark", 3, "A-0-0-2-L2-R"),
        "2,3,Spark,Test Set,TST,2,NM,German,,,Regular,1.50,=HYPERLINK(x),A-0-0-2-L2-R,Funke,,,,Common,",
    );
    keep(&app, csv).await;
    let (router, cookie) = mover(&app).await;
    let (status, created) = make_list(
        &router,
        &cookie,
        json!({ "kind": "consolidation", "threshold": 20 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let list = &created["list"];
    assert_eq!(list["outdated"], false);
    assert_eq!(list["summary"]["moves"], 3);
    let moves = list["moves"].as_array().unwrap();
    let spark = moves.iter().find(|m| m["name"] == "Spark").unwrap();
    assert_eq!(spark["local_name"], "Funke");
    assert_eq!(spark["from_location"], "A-0-0-2-L2-R");
    assert_eq!(spark["to_location"], "A-0-0-1-L2-R");
    assert_eq!(spark["image"], "/api/bins/image/tst/2");
    assert_eq!(spark["done"], false);

    let (status, _, body) =
        call(&router, &cookie, Method::GET, "/api/bins/list/export", None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "nothing_ticked");

    let tick = |ids: Value, done: bool| json!({ "ids": ids, "done": done });
    let (status, _, body) = call(
        &router,
        &cookie,
        Method::PUT,
        "/api/bins/list/ticks",
        Some(tick(json!([spark["id"]]), true)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ticked"], 1);
    let (status, _, body) = call(
        &router,
        &cookie,
        Method::PUT,
        "/api/bins/list/ticks",
        Some(tick(json!(["0123456789abcdef"]), true)),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "unknown_move");
    let (status, _, _) = call(
        &router,
        &cookie,
        Method::PUT,
        "/api/bins/list/ticks",
        Some(tick(json!([]), true)),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (_, list) = get(&router, &cookie, "/api/bins/list").await;
    assert_eq!(list["ticked"], 1);
    let (status, headers, body) =
        call(&router, &cookie, Method::GET, "/api/bins/list/export", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"bin_consolidation_moved.csv\""
    );
    let csv = body.as_str().unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 2, "header and the one ticked pile: {csv}");
    assert!(lines[0].starts_with("cardmarketId,quantity,name"));
    assert!(lines[1].contains("=HYPERLINK(x)"), "exact: {}", lines[1]);
    assert!(lines[1].contains("1.50"));
    assert!(lines[1].contains("A-0-0-1-L2-R"));
    assert!(!lines[1].contains("A-0-0-2-L2-R"));

    let actions = app.audit_actions().await;
    assert_eq!(actions[0].0, "bins.list_created");
    assert_eq!(actions[0].1, "consolidation");
}

#[tokio::test]
async fn after_a_new_import_the_list_must_be_made_again() {
    let app = TestApp::new();
    keep(&app, sample_csv()).await;
    let (router, cookie) = mover(&app).await;
    let (_, created) = make_list(&router, &cookie, json!({ "kind": "consolidation" })).await;
    assert_eq!(created["list"]["threshold"], 20, "the default");
    let ids: Vec<Value> = created["list"]["moves"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].clone())
        .collect();
    call(
        &router,
        &cookie,
        Method::PUT,
        "/api/bins/list/ticks",
        Some(json!({ "ids": ids, "done": true })),
    )
    .await;

    // The next import: the Embers grew, their bin stays.
    keep(
        &app,
        sample_csv().replace(
            &row(3, "Ember", 2, "A-0-0-3-L2-R"),
            &row(3, "Ember", 30, "A-0-0-3-L2-R"),
        ),
    )
    .await;
    let (_, list) = get(&router, &cookie, "/api/bins/list").await;
    assert_eq!(list["outdated"], true);
    assert_eq!(list["moves"], json!([]));
    assert_eq!(list["summary"], Value::Null);
    assert_eq!(list["ticked"], 3);
    let (status, _, body) =
        call(&router, &cookie, Method::GET, "/api/bins/list/export", None).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "never export an old file's rows"
    );
    assert_eq!(body["error"]["code"], "outdated");
    let (status, _, _) = call(
        &router,
        &cookie,
        Method::PUT,
        "/api/bins/list/ticks",
        Some(json!({ "ids": [ids[0]], "done": false })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, again) = make_list(&router, &cookie, json!({ "kind": "consolidation" })).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["carried_ticks"], 1, "the Spark move is still in it");
    assert_eq!(again["dropped_ticks"], 2);
    assert_eq!(again["list"]["outdated"], false);
    assert_eq!(again["list"]["ticked"], 1);
}

#[tokio::test]
async fn lists_are_per_user_and_can_be_discarded() {
    let app = TestApp::new();
    keep(&app, sample_csv()).await;
    let (router, cookie) = mover(&app).await;
    let (_, other) = user(&app, "other", &["analysis.view", "bins.use"]).await;
    make_list(&router, &cookie, json!({ "kind": "defrag" })).await;
    let (_, mine) = get(&router, &cookie, "/api/bins/list").await;
    assert_eq!(mine["kind"], "defrag");
    assert_eq!(mine["threshold"], Value::Null);
    let (_, theirs) = get(&router, &other, "/api/bins/list").await;
    assert_eq!(theirs, Value::Null);

    let (status, _, _) = call(&router, &cookie, Method::DELETE, "/api/bins/list", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = call(&router, &cookie, Method::DELETE, "/api/bins/list", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(app.audit_actions().await[0].0, "bins.list_discarded");
}

#[tokio::test]
async fn settings_out_of_range_and_empty_plans_are_refused() {
    let app = TestApp::new();
    keep(&app, sample_csv()).await;
    let (router, cookie) = mover(&app).await;
    let (status, body) = make_list(
        &router,
        &cookie,
        json!({ "kind": "consolidation", "threshold": 61 }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_threshold");
    let (status, body) = make_list(
        &router,
        &cookie,
        json!({ "kind": "consolidation", "threshold": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "no_moves");
    let (status, _) = make_list(&router, &cookie, json!({ "kind": "everything" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn analysts_without_bin_moves_see_the_analysis_only() {
    let app = TestApp::new();
    keep(&app, sample_csv()).await;
    let (router, cookie) = user(&app, "analyst", &["analysis.view"]).await;
    let (status, _) = get(&router, &cookie, "/api/bins/fragmented").await;
    assert_eq!(status, StatusCode::OK);
    for path in [
        "/api/bins/list",
        "/api/bins/list/export",
        "/api/bins/image/tst/2",
    ] {
        let (status, _) = get(&router, &cookie, path).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]
async fn a_plan_larger_than_a_list_can_hold_is_refused() {
    let app = TestApp::new();
    // 5,001 cards, each spread over two bins: 5,001 defrag moves.
    let mut csv = format!("{HEADER}\n");
    for i in 1..=5_001u32 {
        csv.push_str(&row(
            i,
            "Card",
            1,
            &format!("A-0-{}-{}-L1-R", i / 1000, i % 1000),
        ));
        csv.push('\n');
        csv.push_str(&row(
            i,
            "Card",
            1,
            &format!("B-0-{}-{}-L1-R", i / 1000, i % 1000),
        ));
        csv.push('\n');
    }
    keep(&app, csv).await;
    let (router, cookie) = mover(&app).await;
    let (_, body) = get(&router, &cookie, "/api/bins/fragmented").await;
    assert_eq!(body["defrag"]["moves"], 5_001);
    assert_eq!(body["defrag"]["too_large"], true);
    assert_eq!(body["variants"].as_array().unwrap().len(), 200);
    let (status, body) = make_list(&router, &cookie, json!({ "kind": "defrag" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "too_many_moves");
}
