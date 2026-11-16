use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use crate::bins::tests::{keep_file, HEADER};
use crate::bulk::BulkService;
use axum::http::{header, Method, StatusCode};
use axum::Router;
use check_stock_core::category::DEFAULT_CATEGORY;
use check_stock_core::inventory_db::sync_inventory_on_conn;
use check_stock_core::io::parse_csv;
use serde_json::{json, Value};

const ALL: [&str; 3] = ["bulk_lot.build", "bulk_lot.finish", "bulk_lot.manage"];

/// Six commons (two copies each, €0.10, bins A-0-0-1…6) and a rare, in
/// stock since 2025 (old enough for the default 90 days).
fn export() -> String {
    let mut csv = format!("{HEADER}\n");
    for i in 1..=6 {
        csv.push_str(&format!(
            "{i},2,Common {i},Test Set,TST,{i},NM,English,,,Regular,0.10,,A-0-0-{i}-L3-R,,,,,Common,\n"
        ));
    }
    csv.push_str(
        "7,1,Rare Card,Test Set,TST,7,NM,English,,,Regular,0.10,,A-0-0-7-L3-R,,,,,Rare,\n",
    );
    csv
}

async fn stocked() -> TestApp {
    let app = TestApp::new();
    app.state
        .db
        .write(|conn| {
            let cards = parse_csv(export().as_bytes(), 100).unwrap();
            sync_inventory_on_conn(conn, &cards, DEFAULT_CATEGORY, "2025-01-01")?;
            Ok(keep_file(conn, &export()))
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

fn spec(size: u32) -> Value {
    json!({
        "size": size, "rarities": ["common"], "card_type": "any", "exclude_basic_lands": true,
        "foil": "non_foil", "excluded_conditions": [], "excluded_languages": [],
        "max_price": 0.25, "min_age_days": 90, "max_copies_per_name": 1, "hits": 0,
        "hit_min_price": 0.5, "order": "nearby", "seed": 1
    })
}

async fn draft(router: &Router, cookie: &str, size: u32) -> Value {
    let (status, body) = call(
        router,
        cookie,
        Method::POST,
        "/api/bulk-lots/draft",
        Some(json!({ "spec": spec(size) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn piles(draft: &Value) -> Vec<Value> {
    draft["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["pile"].clone())
        .collect()
}

async fn pick_all(router: &Router, cookie: &str, draft: &Value) {
    let (status, body) = call(
        router,
        cookie,
        Method::PUT,
        "/api/bulk-lots/draft/picked",
        Some(json!({ "piles": piles(draft), "picked": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn finish(router: &Router, cookie: &str) -> (StatusCode, Value) {
    call(
        router,
        cookie,
        Method::POST,
        "/api/bulk-lots/finish",
        Some(json!({ "name": "4 Commons", "price": 2.0 })),
    )
    .await
}

fn backups(app: &TestApp) -> usize {
    std::fs::read_dir(app.dir.path())
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("inventory.db.bulklot-"))
        .count()
}

#[tokio::test]
async fn options_and_preview_follow_the_rules() {
    let app = stocked().await;
    let (router, cookie) = user(&app, "builder", &["bulk_lot.build"]).await;
    let (status, options) = call(
        &router,
        &cookie,
        Method::GET,
        "/api/bulk-lots/options",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    assert_eq!(options["default_spec"]["size"], 100);
    assert_eq!(options["presets"][0]["label"], "100 commons");
    assert_eq!(options["languages"][0]["language"], "English");
    assert_eq!(options["conditions"][0], "NM");

    let (status, preview) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/bulk-lots/preview",
        Some(json!({ "spec": spec(4) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["summary"]["copies"], 4);
    assert_eq!(preview["size"], 4);
    assert_eq!(
        preview["lines"].as_array().unwrap().len(),
        4,
        "one per name"
    );
    assert_eq!(
        preview["lines"][0]["location"], "A-0-0-1-L3-R",
        "closest together, shelf order"
    );
    assert_eq!(preview["types_needed"], 0);
    assert_eq!(preview["default_name"], "4 Commons");
    // Too big a lot names the rule that limits it.
    let (_, big) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/bulk-lots/preview",
        Some(json!({ "spec": spec(50) })),
    )
    .await;
    assert_eq!(big["summary"]["copies"], 6);
    assert!(big["funnel"]["bottleneck"].is_string());

    for bad in [
        json!({ "spec": spec(0) }),
        json!({ "spec": { "size": 4 } }),
        json!({ "spec": spec(4), "extra": 1 }),
    ] {
        let (status, _) = call(
            &router,
            &cookie,
            Method::POST,
            "/api/bulk-lots/preview",
            Some(bad),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
}

#[tokio::test]
async fn a_draft_is_picked_replaced_and_finished_with_backup_csv_and_audit() {
    let app = stocked().await;
    let (router, cookie) = user(&app, "seller", &ALL).await;
    let made = draft(&router, &cookie, 4).await;
    assert_eq!(made["lines"].as_array().unwrap().len(), 4);

    // Not picked yet: nothing to record.
    let (status, body) = finish(&router, &cookie).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "nothing_picked");

    // A card that cannot be found is replaced by the next one.
    let gone = piles(&made)[1].clone();
    let (status, replaced) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/bulk-lots/draft/replace",
        Some(json!({ "pile": gone })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replaced}");
    assert_eq!(replaced["added"], 1);
    let after = &replaced["draft"];
    assert!(!piles(after).contains(&gone));
    assert_eq!(after["summary"]["copies"], 4);

    let (_, current) = call(&router, &cookie, Method::GET, "/api/bulk-lots/draft", None).await;
    pick_all(&router, &cookie, &current).await;
    let (_, info) = call(&router, &cookie, Method::GET, "/api/data-info", None).await;
    let before = info["inventory_version"].as_i64().unwrap();
    let (status, done) = finish(&router, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["copies"], 4);
    assert_eq!(done["data_version"], before + 1);
    assert_eq!(backups(&app), 1);
    let (_, draft_after) = call(&router, &cookie, Method::GET, "/api/bulk-lots/draft", None).await;
    assert_eq!(draft_after, Value::Null, "the draft is used up");

    let id = done["id"].as_i64().unwrap();
    let (status, headers, csv) = send(
        &router,
        request(
            Method::GET,
            &format!("/api/bulk-lots/{id}/csv"),
            None,
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .ends_with(".csv\""));
    let lines: Vec<&str> = csv.as_str().unwrap().lines().collect();
    assert_eq!(lines.len(), 5);
    assert!(lines[1..].iter().all(|l| l.contains(",-1,")), "{lines:?}");

    let (_, history) = call(&router, &cookie, Method::GET, "/api/bulk-lots", None).await;
    assert_eq!(history[0]["status"], "awaiting_import");
    assert_eq!(history[0]["price"], 2.0);
    assert_eq!(history[0]["pending_copies"], 4);
    assert_eq!(history[0]["csv_available"], true);
    let actions = app.audit_actions().await;
    assert_eq!(actions[0].0, "bulk_lot.finished");
    assert_eq!(actions[0].2["cards"].as_array().unwrap().len(), 4);

    // The copies are reserved: a new lot cannot take them again.
    let (_, preview) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/bulk-lots/preview",
        Some(json!({ "spec": spec(4) })),
    )
    .await;
    assert_eq!(preview["summary"]["copies"], 4);
    assert_eq!(preview["funnel"]["reserved"], 4);
}

#[tokio::test]
async fn copies_taken_by_another_lot_meanwhile_are_refused_and_replaced() {
    let app = stocked().await;
    let (router, anna) = user(&app, "anna", &ALL).await;
    let (_, ben) = user(&app, "ben", &ALL).await;
    // Both choose the same commons (one copy each of 6 piles with 2 copies:
    // ask for 6 with up to 2 per name so both take all copies of 3 piles).
    let both = |size| {
        let mut s = spec(size);
        s["max_copies_per_name"] = json!(2);
        s
    };
    let (_, a) = call(
        &router,
        &anna,
        Method::POST,
        "/api/bulk-lots/draft",
        Some(json!({ "spec": both(6) })),
    )
    .await;
    let (_, b) = call(
        &router,
        &ben,
        Method::POST,
        "/api/bulk-lots/draft",
        Some(json!({ "spec": both(6) })),
    )
    .await;
    assert_eq!(piles(&a), piles(&b), "the same cards");
    pick_all(&router, &anna, &a).await;
    pick_all(&router, &ben, &b).await;
    let (status, _) = finish(&router, &anna).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = finish(&router, &ben).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "taken");
    assert_eq!(body["error"]["message"], "3 picked cards are no longer free (another lot or a discard took the copies); replace them");

    let (_, view) = call(&router, &ben, Method::GET, "/api/bulk-lots/draft", None).await;
    let taken: Vec<Value> = view["lines"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["free"].as_i64() < l["copies"].as_i64())
        .map(|l| l["pile"].clone())
        .collect();
    assert_eq!(taken.len(), 3);
    let mut last = Value::Null;
    for pile in taken {
        let (_, replaced) = call(
            &router,
            &ben,
            Method::POST,
            "/api/bulk-lots/draft/replace",
            Some(json!({ "pile": pile })),
        )
        .await;
        last = replaced["draft"].clone();
    }
    pick_all(&router, &ben, &last).await;
    let (status, body) = finish(&router, &ben).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn prices_are_corrected_with_a_check_and_hidden_from_builders() {
    let app = stocked().await;
    let (router, seller) = user(&app, "seller", &ALL).await;
    let made = draft(&router, &seller, 2).await;
    pick_all(&router, &seller, &made).await;
    let (_, done) = finish(&router, &seller).await;
    let id = done["id"].as_i64().unwrap();

    let path = format!("/api/bulk-lots/{id}/price");
    let (status, _) = call(
        &router,
        &seller,
        Method::PUT,
        &path,
        Some(json!({ "price": 3.0, "expected": 2.0 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(
        &router,
        &seller,
        Method::PUT,
        &path,
        Some(json!({ "price": 9.0, "expected": 2.0 })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"]["message"].as_str().unwrap().contains("3.00"));
    let (status, _) = call(
        &router,
        &seller,
        Method::PUT,
        &path,
        Some(json!({ "price": 100001.0, "expected": 3.0 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let price_change = app
        .audit_actions()
        .await
        .into_iter()
        .find(|a| a.0 == "bulk_lot.price_changed")
        .unwrap();
    assert_eq!(
        (
            price_change.2["before"].as_f64(),
            price_change.2["after"].as_f64()
        ),
        (Some(2.0), Some(3.0))
    );

    let (_, builder) = user(&app, "builder", &["bulk_lot.build"]).await;
    let (_, history) = call(&router, &builder, Method::GET, "/api/bulk-lots", None).await;
    assert_eq!(history[0]["price"], Value::Null);
    assert_eq!(history[0]["listed_value"], Value::Null);
    let (status, _) = call(
        &router,
        &builder,
        Method::PUT,
        &path,
        Some(json!({ "price": 1.0, "expected": 3.0 })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, cards) = call(
        &router,
        &builder,
        Method::GET,
        &format!("/api/bulk-lots/{id}/cards"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cards.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn undoing_after_an_import_took_copies_out_needs_their_number() {
    let app = stocked().await;
    let (router, seller) = user(&app, "seller", &ALL).await;
    let made = draft(&router, &seller, 2).await;
    pick_all(&router, &seller, &made).await;
    let (_, done) = finish(&router, &seller).await;
    let id = done["id"].as_i64().unwrap();

    // An import without the two lot copies (Common 1 and Common 2 at 1 left).
    app.state
        .db
        .write(|conn| {
            let csv = export()
                .replace("1,2,Common 1,", "1,1,Common 1,")
                .replace("2,2,Common 2,", "2,1,Common 2,");
            let cards = parse_csv(csv.as_bytes(), 100).unwrap();
            sync_inventory_on_conn(conn, &cards, DEFAULT_CATEGORY, "2026-11-01")
        })
        .await
        .unwrap();
    let (_, history) = call(&router, &seller, Method::GET, "/api/bulk-lots", None).await;
    assert_eq!(history[0]["pending_copies"], 0);
    assert_eq!(history[0]["status"], "done");

    let path = format!("/api/bulk-lots/{id}/undo");
    let (status, body) = call(&router, &seller, Method::POST, &path, Some(json!({}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "confirmation_needed");
    let (status, body) = call(
        &router,
        &seller,
        Method::POST,
        &path,
        Some(json!({ "confirm_removed": "1" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "confirmation_wrong");
    let (status, body) = call(
        &router,
        &seller,
        Method::POST,
        &path,
        Some(json!({ "confirm_removed": "2" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, history) = call(&router, &seller, Method::GET, "/api/bulk-lots", None).await;
    assert_eq!(history, json!([]));
    assert_eq!(app.audit_actions().await[0].0, "bulk_lot.undone");
    let (status, _) = call(
        &router,
        &seller,
        Method::GET,
        &format!("/api/bulk-lots/{id}/csv"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "its CSV went with it");
}

#[tokio::test]
async fn a_lot_awaiting_its_import_is_undone_or_marked_removed_directly() {
    let app = stocked().await;
    let (router, seller) = user(&app, "seller", &ALL).await;
    for _ in 0..2 {
        let made = draft(&router, &seller, 2).await;
        pick_all(&router, &seller, &made).await;
        finish(&router, &seller).await;
    }
    let (_, history) = call(&router, &seller, Method::GET, "/api/bulk-lots", None).await;
    let (newest, older) = (
        history[0]["id"].as_i64().unwrap(),
        history[1]["id"].as_i64().unwrap(),
    );
    let (status, _) = call(
        &router,
        &seller,
        Method::POST,
        &format!("/api/bulk-lots/{newest}/undo"),
        Some(json!({})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "nothing taken out yet: no number needed"
    );

    let removed = format!("/api/bulk-lots/{older}/removed");
    let (status, _) = call(&router, &seller, Method::POST, &removed, None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(&router, &seller, Method::POST, &removed, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "nothing_pending");
    assert_eq!(backups(&app), 4, "two finishes, the undo and the removal");
    let actions: Vec<String> = app.audit_actions().await.into_iter().map(|a| a.0).collect();
    assert!(actions.contains(&"bulk_lot.marked_removed".to_string()));
}

#[tokio::test]
async fn reserved_copies_cannot_be_discarded() {
    let app = stocked().await;
    let (router, seller) = user(
        &app,
        "seller",
        &[ALL[0], ALL[1], ALL[2], "search.use", "inventory.discard"],
    )
    .await;
    let mut s = spec(2);
    s["max_copies_per_name"] = json!(2);
    let (_, made) = call(
        &router,
        &seller,
        Method::POST,
        "/api/bulk-lots/draft",
        Some(json!({ "spec": s })),
    )
    .await;
    pick_all(&router, &seller, &made).await;
    let pile = piles(&made)[0].clone();
    assert_eq!(made["lines"][0]["copies"], 2);
    let (status, _) = finish(&router, &seller).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(
        &router,
        &seller,
        Method::POST,
        "/api/inventory/discard",
        Some(json!({ "items": [{ "pile": pile, "quantity": 1 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["error"]["message"],
        "Only 0 copies of \"Common 1\" are left"
    );
}

#[tokio::test]
async fn card_types_are_looked_up_on_scryfall_for_the_stock() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let scryfall = MockServer::start().await;
    let card = |cn: &str, type_line: &str| {
        json!({ "id": format!("tst-{cn}"), "name": "Card", "set": "tst", "set_name": "Test Set",
                "collector_number": cn, "rarity": "common", "type_line": type_line })
    };
    Mock::given(method("POST"))
        .and(path("/cards/collection"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                card("1", "Creature — Elf"), card("2", "Instant"), card("3", "Creature — Goblin"),
                card("4", "Land"), card("5", "Sorcery"), card("6", "Creature — Human"),
            ]
        })))
        .expect(1)
        .mount(&scryfall)
        .await;
    let app = stocked().await.with(|state, dir| {
        state.with_bulk(BulkService::new(&dir.join("cache")).with_scryfall(&scryfall.uri()))
    });
    let (router, cookie) = user(&app, "builder", &["bulk_lot.build"]).await;
    let mut creatures = spec(10);
    creatures["card_type"] = json!("creature");
    let body = json!({ "spec": creatures });
    let (_, before) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/bulk-lots/preview",
        Some(body.clone()),
    )
    .await;
    assert_eq!(before["types_needed"], 6);
    assert_eq!(before["summary"]["copies"], 0);
    let (status, _) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/bulk-lots/types/load",
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    for _ in 0..200 {
        let (_, status) = call(
            &router,
            &cookie,
            Method::GET,
            "/api/bulk-lots/types/status",
            None,
        )
        .await;
        if status["state"] == "finished" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let (_, after) = call(
        &router,
        &cookie,
        Method::POST,
        "/api/bulk-lots/preview",
        Some(body),
    )
    .await;
    assert_eq!(after["types_needed"], 0);
    assert_eq!(after["summary"]["copies"], 3, "the three creatures");
}

/// Without a backup nothing is recorded. The data folder is made read-only:
/// the database files stay writable, but no backup file can be created.
#[cfg(unix)]
#[tokio::test]
async fn without_a_backup_no_lot_is_recorded() {
    use std::os::unix::fs::PermissionsExt;
    let app = stocked().await;
    let (router, seller) = user(&app, "seller", &ALL).await;
    let made = draft(&router, &seller, 2).await;
    pick_all(&router, &seller, &made).await;
    let dir = app.dir.path().to_path_buf();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Running as root ignores the permissions: nothing to check then.
    let probe = std::fs::write(dir.join("probe"), "x").is_ok();
    let (status, body) = finish(&router, &seller).await;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    if probe {
        return;
    }
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"]["code"], "backup_failed");
    let (_, history) = call(&router, &seller, Method::GET, "/api/bulk-lots", None).await;
    assert_eq!(history, json!([]), "nothing recorded");
    let (_, still) = call(&router, &seller, Method::GET, "/api/bulk-lots/draft", None).await;
    assert!(still.is_object(), "the draft is kept to try again");
}
