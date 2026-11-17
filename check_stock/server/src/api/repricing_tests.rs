use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use crate::bins::tests::{keep_file, HEADER};
use crate::market::tests::fake_tracker;
use crate::market::MarketService;
use crate::repricing::tests::follow_trend;
use axum::http::{header, Method, StatusCode};
use axum::Router;
use check_stock_core::api::inventory_sync::InventorySyncClient;
use check_stock_core::category::DEFAULT_CATEGORY;
use check_stock_core::inventory_db::{record_bulk_lot_in_tx, sync_inventory_on_conn};
use check_stock_core::io::parse_csv;
use check_stock_core::repricing::Ruleset;
use serde_json::{json, Value};
use std::time::Duration;

const VIEW: &str = "repricing.view";
const EDIT: &str = "repricing.edit_rules";
const EXPORT: &str = "repricing.export";

/// Riser (2 × €9, trend 10), Faller (1 × €12, trend 10), Flat (4 × €5,
/// trend 5) and a card the tracker does not know.
fn export() -> String {
    format!(
        "{HEADER}\n\
         101,2,Riser,Test Set,TST,1,NM,English,,,Regular,9.00,,A-0-0-1-L3-R,,,,,Rare,\n\
         102,1,Faller,Test Set,TST,2,NM,English,,,Regular,12.00,,A-0-0-2-L3-R,,,,,Rare,\n\
         103,4,Flat,Test Set,TST,3,NM,English,,,Regular,5.00,,A-0-0-3-L3-R,,,,,Rare,\n\
         104,1,Unknown,Test Set,TST,4,NM,English,,,Regular,1.00,,A-0-0-4-L3-R,,,,,Common,\n"
    )
}

async fn stock(app: &TestApp, csv: String) {
    app.state
        .db
        .write(move |conn| {
            let cards = parse_csv(csv.as_bytes(), 100).unwrap();
            sync_inventory_on_conn(conn, &cards, DEFAULT_CATEGORY, "2025-01-01")?;
            Ok(keep_file(conn, &csv))
        })
        .await
        .unwrap();
}

/// The stock above and the fake tracker (not loaded yet).
async fn stocked(tracker: &wiremock::MockServer) -> TestApp {
    let url = tracker.uri();
    let app = TestApp::new().with(|state, _| {
        state.with_market(
            MarketService::new(Some(InventorySyncClient::new(&url))).with_cooldown(Duration::ZERO),
        )
    });
    stock(&app, export()).await;
    app
}

async fn user(app: &TestApp, name: &str, keys: &[&str]) -> (Router, String) {
    app.add_user(name, keys).await;
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

async fn load_market(app: &TestApp, router: &Router, cookie: &str) {
    let (status, body) = call(
        router,
        cookie,
        Method::POST,
        "/api/repricing/market/load",
        Some(json!({ "refresh": false })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while app.state.market.loaded().is_none() {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn rules(percent: f64) -> Value {
    let mut r = follow_trend();
    r.rules[0].action.percent = percent;
    serde_json::to_value(r).unwrap()
}

async fn put_draft(
    router: &Router,
    cookie: &str,
    base: i64,
    ruleset: Value,
) -> (StatusCode, Value) {
    call(
        router,
        cookie,
        Method::PUT,
        "/api/repricing/draft",
        Some(json!({ "base_revision": base, "ruleset": ruleset })),
    )
    .await
}

async fn publish(router: &Router, cookie: &str, expected: i64) -> (StatusCode, Value) {
    call(
        router,
        cookie,
        Method::POST,
        "/api/repricing/publish",
        Some(json!({ "expected_revision": expected })),
    )
    .await
}

/// Publishes the trend rule (×`percent`) as an editor called "boss".
async fn published(app: &TestApp, percent: f64) {
    let (router, cookie) = user(app, "boss", &[VIEW, EDIT]).await;
    let (status, body) = put_draft(&router, &cookie, 0, rules(percent)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = publish(&router, &cookie, 0).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn review(router: &Router, cookie: &str, query: &str) -> Value {
    let (status, body) = call(
        router,
        cookie,
        Method::GET,
        &format!("/api/repricing/review?{query}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn row<'a>(page: &'a Value, name: &str) -> &'a Value {
    page["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap_or_else(|| panic!("no {name} in {page}"))
}

fn names(page: &Value) -> Vec<&str> {
    page["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect()
}

const ALL_BY_NAME: &str = "filter=all&sort=name&desc=false";

#[tokio::test]
async fn setup_starts_with_the_starter_rules() {
    let tracker = fake_tracker().await;
    let app = stocked(&tracker).await;
    let (router, cookie) = user(&app, "viewer", &[VIEW]).await;
    let (status, setup) = call(&router, &cookie, Method::GET, "/api/repricing/setup", None).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    assert_eq!(setup["published"]["revision"], 0);
    assert_eq!(setup["published"]["ruleset"]["name"], "My repricing rules");
    assert_eq!(setup["draft"], Value::Null);
    assert_eq!(setup["source"]["file_name"], "inventory-report-Magic.csv");
    let options = &setup["options"];
    assert_eq!(options["rarities"][0], "Rare", "most copies first");
    assert_eq!(options["fields"].as_array().unwrap().len(), 6);
    assert_eq!(
        options["fields"][0],
        json!({ "key": "Trend", "label": "Trend" })
    );
    assert_eq!(options["condition_templates"][0]["label"], "Rarity");
    assert_eq!(
        options["condition_templates"][0]["condition"],
        json!({ "Rarity": [] })
    );
    assert!(options["rule_templates"].as_array().unwrap().len() > 3);
    assert_eq!(options["conditions"][0], "NM");
}

#[tokio::test]
async fn without_an_import_there_is_nothing_to_review() {
    let app = TestApp::new();
    let (router, cookie) = user(&app, "viewer", &[VIEW]).await;
    let (status, setup) = call(&router, &cookie, Method::GET, "/api/repricing/setup", None).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    assert_eq!(setup["source"], Value::Null);
    assert_eq!(setup["options"]["languages"], json!(["English", "German"]));
    let (status, body) = call(&router, &cookie, Method::GET, "/api/repricing/review", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "no_import");
}

#[tokio::test]
async fn the_review_prices_the_stock_by_the_published_rules() {
    let tracker = fake_tracker().await;
    let app = stocked(&tracker).await;
    published(&app, 100.0).await;
    let (router, cookie) = user(&app, "viewer", &[VIEW]).await;
    load_market(&app, &router, &cookie).await;

    let page = review(&router, &cookie, ALL_BY_NAME).await;
    assert_eq!(names(&page), ["Faller", "Flat", "Riser", "Unknown"]);
    let riser = row(&page, "Riser");
    assert_eq!(
        (&riser["old_price"], &riser["new_price"], &riser["outcome"]),
        (&json!(9.0), &json!(10.0), &json!("raise"))
    );
    assert_eq!(riser["ticked"], true);
    assert_eq!(riser["quantity"], 2);
    assert_eq!(riser["collector_number"], "1");
    assert_eq!(row(&page, "Faller")["outcome"], "lower");
    assert_eq!(row(&page, "Flat")["outcome"], "same");
    assert_eq!(row(&page, "Unknown")["outcome"], "no_market_data");
    assert_eq!(row(&page, "Unknown")["outcome_label"], "No market price");
    let s = &page["summary"];
    assert_eq!(
        (&s["raises"], &s["lowers"], &s["selected"]),
        (&json!(1), &json!(1), &json!(2))
    );
    assert_eq!(s["selected_copies"], 3);
    assert_eq!(s["value_before"], 51.0);
    assert_eq!(s["value_after"], 51.0);
    assert_eq!(page["revision"], 1);
    assert_eq!(page["draft"], false);
    assert_eq!(page["market"]["loaded"], true);
    assert_eq!(page["market"]["covered"], 3);
    assert_eq!(page["market"]["newest_price_date"], "2026-11-15");
    let rule = &page["rules"][0];
    assert_eq!(rule["name"], "Follow the trend");
    assert_eq!(
        (&rule["matched"], &rule["raised"], &rule["lowered"]),
        (&json!(4), &json!(1), &json!(1))
    );
    assert_eq!(rule["when"], "Every listing");

    // Default view: changes by impact (both €2 here: then by name); paging.
    let page = review(&router, &cookie, "").await;
    assert_eq!(names(&page), ["Faller", "Riser"]);
    let page = review(&router, &cookie, "sort=change").await;
    assert_eq!(names(&page), ["Riser", "Faller"], "+11% before −17%");
    let page = review(&router, &cookie, &format!("{ALL_BY_NAME}&offset=1&limit=2")).await;
    assert_eq!(
        (names(&page), &page["total"]),
        (vec!["Flat", "Riser"], &json!(4))
    );
    let page = review(&router, &cookie, "filter=all&search=fal,%20unk").await;
    assert_eq!(page["total"], 2);

    // A viewer has no draft to look at.
    let page = review(&router, &cookie, "draft=true").await;
    assert_eq!(page["draft"], false);
    for bad in [
        "nope=1",
        "rule=5",
        "filter=sideways",
        &format!("search={}", "x".repeat(201)),
    ] {
        let (status, body) = call(
            &router,
            &cookie,
            Method::GET,
            &format!("/api/repricing/review?{bad}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {body}");
    }
}

#[tokio::test]
async fn drafts_are_private_until_published() {
    let tracker = fake_tracker().await;
    let app = stocked(&tracker).await;
    let (anna, anna_cookie) = user(&app, "anna", &[VIEW, EDIT]).await;
    let (ben, ben_cookie) = user(&app, "ben", &[VIEW, EDIT]).await;
    let (viewer, viewer_cookie) = user(&app, "viewer", &[VIEW]).await;
    load_market(&app, &viewer, &viewer_cookie).await;

    let (status, saved) = put_draft(&anna, &anna_cookie, 0, rules(50.0)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["draft"]["base_revision"], 0);
    let mine = review(
        &anna,
        &anna_cookie,
        "draft=true&filter=all&sort=name&desc=false",
    )
    .await;
    assert_eq!(mine["draft"], true);
    assert_eq!(row(&mine, "Riser")["new_price"], 5.0);
    // Anna's published view, Ben and the viewer still see the starter rules.
    for (router, cookie, query) in [
        (&anna, &anna_cookie, ALL_BY_NAME),
        (&ben, &ben_cookie, "draft=true&filter=all"),
        (&viewer, &viewer_cookie, "draft=true&filter=all"),
    ] {
        let page = review(router, cookie, query).await;
        assert_eq!(page["draft"], false);
        assert_eq!(page["rules"][0]["name"], "Protect expensive cards");
    }

    // A draft equal to the published rules is no draft.
    let starter = serde_json::to_value(Ruleset::starter()).unwrap();
    let (_, saved) = put_draft(&ben, &ben_cookie, 0, starter).await;
    assert_eq!(saved["draft"], Value::Null);

    let (_, _) = put_draft(&ben, &ben_cookie, 0, rules(80.0)).await;
    let (status, body) = publish(&anna, &anna_cookie, 0).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["revision"], 1);
    assert_eq!(body["updated_by"], "anna");
    let (_, setup) = call(
        &anna,
        &anna_cookie,
        Method::GET,
        "/api/repricing/setup",
        None,
    )
    .await;
    assert_eq!(setup["draft"], Value::Null, "published = no draft left");
    let page = review(&viewer, &viewer_cookie, ALL_BY_NAME).await;
    assert_eq!(row(&page, "Riser")["new_price"], 5.0);

    // Ben started from revision 0: no silent overwrite.
    let (status, body) = publish(&ben, &ben_cookie, 0).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "ruleset_conflict");
    assert!(
        body["error"]["message"].as_str().unwrap().contains("anna"),
        "{body}"
    );
    let (status, body) = publish(&ben, &ben_cookie, 1).await;
    assert_eq!((status, &body["revision"]), (StatusCode::OK, &json!(2)));
    let (status, body) = publish(&ben, &ben_cookie, 2).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"]["code"], "no_draft");

    let published: Vec<_> = app
        .audit_actions()
        .await
        .into_iter()
        .filter(|(a, _, _)| a == "repricing.published")
        .collect();
    assert_eq!(published.len(), 2);
    assert_eq!(published[0].1, "2");
    assert_eq!(published[0].2["previous"], 1);
    assert_eq!(published[0].2["based_on"], 0);
    assert_eq!(published[1].2["rule_names"], json!(["Follow the trend"]));
}

#[tokio::test]
async fn drafts_are_checked_and_cleaned() {
    let tracker = fake_tracker().await;
    let app = stocked(&tracker).await;
    let (router, cookie) = user(&app, "anna", &[VIEW, EDIT]).await;
    let mut wild = rules(1e9);
    wild["guardrails"]["floors"]["common"] = json!(-5);
    let (status, saved) = put_draft(&router, &cookie, 0, wild).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        saved["draft"]["ruleset"]["rules"][0]["action"]["percent"],
        1000.0
    );
    assert_eq!(
        saved["draft"]["ruleset"]["guardrails"]["floors"]["common"],
        0.0
    );

    let (status, body) = put_draft(&router, &cookie, 99, rules(90.0)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "invalid_revision");
    let (status, body) = call(
        &router,
        &cookie,
        Method::PUT,
        "/api/repricing/draft",
        Some(json!({ "base_revision": 0, "ruleset": rules(90.0), "extra": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, _) = put_draft(&router, &cookie, 0, json!({ "rules": "no" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Files become the draft; errors name the position, not the content.
    let (status, saved) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/repricing/draft/file",
        Some(json!({ "json": follow_trend().to_json() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["draft"]["ruleset"]["name"], "Trend");
    let (status, body) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/repricing/draft/file",
        Some(json!({ "json": "{\"rules\": \"top secret\"}" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("line 1") && !message.contains("secret"),
        "{message}"
    );
    let big = json!({ "json": "x".repeat(super::RULESET_BODY_LIMIT + 1) });
    let (status, _) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/repricing/draft/file",
        Some(big),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let big = json!({ "json": "x".repeat(crate::repricing::MAX_RULESET_BYTES + 1) });
    let (status, body) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/repricing/draft/file",
        Some(big),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "too_large");

    // Downloads: the draft or the published rules, as a desktop file.
    let (status, headers, body) = send(
        &router,
        request(
            Method::GET,
            "/api/repricing/ruleset.json?draft=true",
            None,
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"repricing_ruleset.json\""
    );
    assert_eq!(
        Ruleset::from_json(&body.to_string()).unwrap(),
        follow_trend()
    );
    let (_, _, body) = send(
        &router,
        request(
            Method::GET,
            "/api/repricing/ruleset.json",
            None,
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(body["name"], "My repricing rules");

    let (status, _) = call(
        &router,
        &cookie,
        Method::DELETE,
        "/api/repricing/draft",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, setup) = call(&router, &cookie, Method::GET, "/api/repricing/setup", None).await;
    assert_eq!(setup["draft"], Value::Null);
}

async fn ticks(router: &Router, cookie: &str, body: Value) -> (StatusCode, Value) {
    call(
        router,
        cookie,
        Method::PUT,
        "/api/repricing/review/ticks",
        Some(body),
    )
    .await
}

#[tokio::test]
async fn ticks_and_hand_prices_are_per_user_and_import() {
    let tracker = fake_tracker().await;
    let app = stocked(&tracker).await;
    published(&app, 100.0).await;
    let (router, cookie) = user(&app, "anna", &[VIEW, EXPORT]).await;
    let (other, other_cookie) = user(&app, "ben", &[VIEW, EXPORT]).await;
    load_market(&app, &router, &cookie).await;
    let page = review(&router, &cookie, ALL_BY_NAME).await;
    let version = page["source"]["version"].clone();
    let riser = row(&page, "Riser")["pile"].clone();
    let flat = row(&page, "Flat")["pile"].clone();

    let (status, body) = ticks(
        &router,
        &cookie,
        json!({ "version": version, "target": { "piles": [riser] }, "ticked": false }),
    )
    .await;
    assert_eq!((status, &body["changed"]), (StatusCode::OK, &json!(1)));
    let page = review(&router, &cookie, ALL_BY_NAME).await;
    assert_eq!(row(&page, "Riser")["ticked"], false);
    assert_eq!(row(&page, "Riser")["overridden"], false);
    assert_eq!(page["summary"]["selected"], 1);
    let theirs = review(&other, &other_cookie, ALL_BY_NAME).await;
    assert_eq!(row(&theirs, "Riser")["ticked"], true);

    let manual = |price: Value, pile: &Value, version: &Value| json!({ "version": version, "pile": pile, "price": price });
    let (status, body) = call(
        &router,
        &cookie,
        Method::PUT,
        "/api/repricing/review/manual",
        Some(manual(json!(6.5), &flat, &version)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let page = review(&router, &cookie, ALL_BY_NAME).await;
    let f = row(&page, "Flat");
    assert_eq!(
        (&f["new_price"], &f["manual"], &f["ticked"]),
        (&json!(6.5), &json!(true), &json!(true))
    );
    for (body, status, code) in [
        (
            manual(json!(0.01), &flat, &version),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_price",
        ),
        (
            manual(json!(100_000.01), &flat, &version),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_price",
        ),
        (
            manual(json!(1), &json!("00000000000000ff"), &version),
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_pile",
        ),
        (
            manual(json!(1), &flat, &json!(999)),
            StatusCode::CONFLICT,
            "stock_changed",
        ),
    ] {
        let (s, b) = call(
            &router,
            &cookie,
            Method::PUT,
            "/api/repricing/review/manual",
            Some(body),
        )
        .await;
        assert_eq!(
            (s, b["error"]["code"].as_str().unwrap()),
            (status, code),
            "{b}"
        );
    }
    let (status, body) = ticks(
        &router,
        &cookie,
        json!({ "version": version, "target": { "piles": ["zz"] }, "ticked": true }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // Untick everything shown, then back to the defaults.
    let shown = json!({ "draft": false, "filter": "changes", "rule": null, "search": "" });
    let (_, body) = ticks(
        &router,
        &cookie,
        json!({ "version": version, "target": { "shown": shown }, "ticked": false }),
    )
    .await;
    assert_eq!(body["changed"], 3);
    assert_eq!(review(&router, &cookie, "").await["summary"]["selected"], 0);
    let (status, _) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/repricing/review/reset",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let page = review(&router, &cookie, ALL_BY_NAME).await;
    assert_eq!(page["summary"]["selected"], 2);
    assert_eq!(row(&page, "Flat")["manual"], false);

    // A new import starts a fresh review and says what was dropped.
    ticks(
        &router,
        &cookie,
        json!({ "version": version, "target": { "piles": [riser] }, "ticked": false }),
    )
    .await;
    stock(&app, export()).await;
    let page = review(&router, &cookie, ALL_BY_NAME).await;
    assert_ne!(page["source"]["version"], version);
    assert_eq!(
        (&page["dropped_ticks"], &page["dropped_manual"]),
        (&json!(1), &json!(0))
    );
    assert_eq!(row(&page, "Riser")["ticked"], true);
}

async fn export_now(router: &Router, cookie: &str, page: &Value) -> (StatusCode, Value) {
    call(
        router,
        cookie,
        Method::POST,
        "/api/repricing/export",
        Some(json!({
            "revision": page["revision"],
            "data_version": page["data_version"],
            "market_version": page["market"]["version"],
        })),
    )
    .await
}

fn csv_of(body: &Value) -> Vec<String> {
    body.as_str().unwrap().lines().map(str::to_string).collect()
}

#[tokio::test]
async fn exports_use_the_published_rules_and_keep_both_files() {
    let tracker = fake_tracker().await;
    let app = stocked(&tracker).await;
    published(&app, 100.0).await;
    let (router, cookie) = user(&app, "anna", &[VIEW, EDIT, EXPORT]).await;
    load_market(&app, &router, &cookie).await;
    // An unpublished draft never reaches an export.
    put_draft(&router, &cookie, 1, rules(50.0)).await;
    let page = review(&router, &cookie, "").await;

    let (status, result) = export_now(&router, &cookie, &page).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(
        (&result["changes"], &result["raises"], &result["lowers"]),
        (&json!(2), &json!(1), &json!(1))
    );
    let file = result["file_name"].as_str().unwrap();
    assert!(
        file.starts_with("repricing_") && file.ends_with(".csv"),
        "{file}"
    );
    assert_eq!(result["undo_file_name"], file.replace(".csv", "_undo.csv"));

    let id = result["id"].as_i64().unwrap();
    let (status, headers, body) = send(
        &router,
        request(
            Method::GET,
            &format!("/api/repricing/exports/{id}/csv"),
            None,
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "text/csv; charset=utf-8");
    let lines = csv_of(&body);
    assert_eq!(lines.len(), 3);
    assert!(
        lines[1].starts_with("101,2,Riser,") && lines[1].contains(",10.00,"),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("102,1,Faller,") && lines[2].contains(",10.00,"),
        "{}",
        lines[2]
    );
    let (_, _, body) = send(
        &router,
        request(
            Method::GET,
            &format!("/api/repricing/exports/{id}/undo"),
            None,
            Some(&cookie),
        ),
    )
    .await;
    let undo = csv_of(&body);
    assert!(
        undo[1].contains(",9.00,") && undo[2].contains(",12.00,"),
        "{undo:?}"
    );

    let (_, list) = call(
        &router,
        &cookie,
        Method::GET,
        "/api/repricing/exports",
        None,
    )
    .await;
    assert_eq!(list[0]["id"], id);
    assert_eq!(
        (&list[0]["username"], &list[0]["revision"]),
        (&json!("anna"), &json!(1))
    );
    let audit = app.audit_actions().await;
    let entry = audit
        .iter()
        .find(|(a, _, _)| a == "repricing.exported")
        .unwrap();
    assert_eq!(
        (entry.1.as_str(), &entry.2["changes"]),
        (id.to_string().as_str(), &json!(2))
    );
    let (status, _) = call(
        &router,
        &cookie,
        Method::GET,
        "/api/repricing/exports/999/csv",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // What the user checked must still hold.
    let mut stale = page.clone();
    stale["revision"] = json!(0);
    let (status, body) = export_now(&router, &cookie, &stale).await;
    assert_eq!(
        (status, &body["error"]["code"]),
        (StatusCode::CONFLICT, &json!("rules_changed"))
    );
    let mut stale = page.clone();
    stale["data_version"] = json!(0);
    let (status, body) = export_now(&router, &cookie, &stale).await;
    assert_eq!(
        (status, &body["error"]["code"]),
        (StatusCode::CONFLICT, &json!("stock_changed"))
    );
    let mut stale = page.clone();
    stale["market"]["version"] = json!(99);
    let (status, _) = export_now(&router, &cookie, &stale).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let shown = json!({ "draft": false, "filter": "changes", "rule": null, "search": "" });
    ticks(
        &router,
        &cookie,
        json!({ "version": page["source"]["version"], "target": { "shown": shown }, "ticked": false }),
    )
    .await;
    let (status, body) = export_now(&router, &cookie, &page).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "nothing_ticked");
}

#[tokio::test]
async fn discarded_and_reserved_copies_never_come_back() {
    let tracker = fake_tracker().await;
    let app = stocked(&tracker).await;
    published(&app, 100.0).await;
    let (router, cookie) = user(
        &app,
        "anna",
        &[VIEW, EXPORT, "inventory.discard", "search.use"],
    )
    .await;
    load_market(&app, &router, &cookie).await;
    let page = review(&router, &cookie, ALL_BY_NAME).await;
    let riser = row(&page, "Riser")["pile"].clone();
    let faller = row(&page, "Faller")["pile"].clone();

    // One of the two Risers is written off.
    let (status, body) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/inventory/discard",
        Some(json!({ "items": [{ "pile": riser, "quantity": 1 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The Faller went into a bulk lot awaiting its import.
    app.state
        .db
        .write(|conn| {
            let cards = parse_csv(export().as_bytes(), 100).unwrap();
            let tx = conn.transaction()?;
            record_bulk_lot_in_tx(
                &tx,
                "Lot",
                1.0,
                &[(cards[1].clone(), 1)],
                DEFAULT_CATEGORY,
                "2026-11-16",
            )?;
            crate::bulk::data_changed(&tx, "bulk_lot.finished", chrono::Utc::now())?;
            tx.commit()
        })
        .await
        .unwrap();

    let page = review(&router, &cookie, ALL_BY_NAME).await;
    assert_eq!(row(&page, "Riser")["quantity"], 1);
    let f = row(&page, "Faller");
    assert_eq!(
        (&f["held_back"], &f["ticked"], &f["outcome"]),
        (&json!(true), &json!(false), &json!("lower"))
    );
    assert_eq!(page["summary"]["held_back"], 1);
    // Ticking it by hand does not change that.
    ticks(
        &router,
        &cookie,
        json!({ "version": page["source"]["version"], "target": { "piles": [faller] }, "ticked": true }),
    )
    .await;
    let page = review(&router, &cookie, ALL_BY_NAME).await;
    assert_eq!(row(&page, "Faller")["ticked"], false);
    let held = review(&router, &cookie, "filter=held_back").await;
    assert_eq!(names(&held), ["Faller"]);

    let (status, result) = export_now(&router, &cookie, &page).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let (_, body) = call(
        &router,
        &cookie,
        Method::GET,
        &format!("/api/repricing/exports/{}/csv", result["id"]),
        None,
    )
    .await;
    let lines = csv_of(&body);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[1].starts_with("101,1,Riser,"), "{}", lines[1]);
}

#[tokio::test]
async fn the_detail_explains_a_price() {
    let tracker = fake_tracker().await;
    let app = stocked(&tracker).await;
    published(&app, 100.0).await;
    let (router, cookie) = user(&app, "anna", &[VIEW, EXPORT]).await;
    load_market(&app, &router, &cookie).await;
    let page = review(&router, &cookie, ALL_BY_NAME).await;
    let riser = row(&page, "Riser")["pile"].as_str().unwrap().to_string();
    let (status, detail) = call(
        &router,
        &cookie,
        Method::GET,
        &format!("/api/repricing/listings/{riser}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["row"]["name"], "Riser");
    assert_eq!(detail["steps"][0], "Rule 1: Follow the trend");
    assert!(detail["steps"].as_array().unwrap().len() > 2, "{detail}");
    assert_eq!(detail["market_date"], "2026-11-15");
    assert_eq!(
        detail["market"][0],
        json!({ "label": "Trend", "price": 10.0 })
    );
    assert_eq!(detail["market"][1]["price"], Value::Null);
    assert!(detail["age_days"].as_i64().unwrap() > 300);
    assert_eq!(detail["manual_price"], Value::Null);

    call(
        &router,
        &cookie,
        Method::PUT,
        "/api/repricing/review/manual",
        Some(json!({ "version": page["source"]["version"], "pile": riser, "price": 7.25 })),
    )
    .await;
    let (_, detail) = call(
        &router,
        &cookie,
        Method::GET,
        &format!("/api/repricing/listings/{riser}"),
        None,
    )
    .await;
    assert_eq!(detail["steps"], json!(["Set by hand to €7.25"]));
    assert_eq!(detail["manual_price"], 7.25);

    for pile in ["..%2Fx", "zz", "0123456789abcdef"] {
        let (status, _) = call(
            &router,
            &cookie,
            Method::GET,
            &format!("/api/repricing/listings/{pile}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{pile}");
    }
}

#[tokio::test]
async fn only_editors_see_their_draft() {
    let tracker = fake_tracker().await;
    let app = stocked(&tracker).await;
    let id = app.add_user("former", &[VIEW]).await;
    // A draft left over from when the user could still edit the rules.
    app.state
        .db
        .write(move |conn| {
            crate::repricing::save_draft(conn, id, 0, &follow_trend(), chrono::Utc::now())
                .map(|_| ())
        })
        .await
        .unwrap();
    let router = app.router();
    let cookie = login_ok(&router, "former", PASSWORD).await;
    let (_, setup) = call(&router, &cookie, Method::GET, "/api/repricing/setup", None).await;
    assert_eq!(setup["draft"], Value::Null);
    let page = review(&router, &cookie, "draft=true").await;
    assert_eq!(page["draft"], false);
    let (_, _, body) = send(
        &router,
        request(
            Method::GET,
            "/api/repricing/ruleset.json?draft=true",
            None,
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(body["name"], "My repricing rules");
}
