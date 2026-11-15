use super::*;
use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use axum::http::Method;
use axum::Router;
use check_stock_core::category::DEFAULT_CATEGORY;
use check_stock_core::inventory_db::sync_inventory_on_conn;
use check_stock_core::models::Card;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

fn card(id: &str, name: &str, qty: &str, price: &str) -> Card {
    Card {
        cardmarket_id: id.to_string(),
        name: name.to_string(),
        quantity: qty.to_string(),
        price: price.to_string(),
        set_code: "TST".to_string(),
        location: Some("A-1-1-1-L2-R".to_string()),
        // No listing date (as in Cardmarket's export): the first import counts.
        listed_at: String::new(),
        ..Card::test_default()
    }
}

/// Bolt: 3 copies at 1.00 sold out in 7 days; Shock: 2 at 5.00 in 14 days;
/// Opt: 1 copy (below the usual minimum of 2); Ancient: still in stock.
async fn inventory(app: &TestApp) {
    app.state
        .db
        .write(|conn| {
            sync_inventory_on_conn(
                conn,
                &[
                    card("101", "Bolt", "3", "1.00"),
                    card("102", "Shock", "2", "5.00"),
                    card("103", "Opt", "1", "0.50"),
                    card("104", "Ancient", "4", "2.00"),
                ],
                DEFAULT_CATEGORY,
                "2026-01-01",
            )?;
            sync_inventory_on_conn(
                conn,
                &[
                    card("101", "Bolt", "0", "1.00"),
                    card("102", "Shock", "2", "5.00"),
                    card("103", "Opt", "0", "0.50"),
                    card("104", "Ancient", "4", "2.00"),
                ],
                DEFAULT_CATEGORY,
                "2026-01-08",
            )?;
            sync_inventory_on_conn(
                conn,
                &[
                    card("102", "Shock", "0", "5.00"),
                    card("104", "Ancient", "4", "2.00"),
                ],
                DEFAULT_CATEGORY,
                "2026-01-15",
            )
            .map(|_| ())
        })
        .await
        .unwrap();
}

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

#[tokio::test]
async fn data_info_says_how_current_the_data_is() {
    let app = TestApp::new();
    let (router, cookie) = user_with(&app, "plain", &[]).await;
    let (status, body) = get(&router, "/api/data-info", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["copy_taken_at"], Value::Null);
    assert_eq!(body["last_import"], Value::Null);

    inventory(&app).await;
    app.state
        .db
        .write(|conn| crate::migrations::set_meta(conn, META_COPY_TAKEN_AT, "2026-01-16T08:00:00Z"))
        .await
        .unwrap();
    let (_, body) = get(&router, "/api/data-info", &cookie).await;
    assert_eq!(body["copy_taken_at"], "2026-01-16T08:00:00Z");
    assert_eq!(body["last_import"], "2026-01-15");
}

#[tokio::test]
async fn restock_ranks_sold_out_cards_without_money_for_analysts() {
    let app = TestApp::new();
    inventory(&app).await;
    let (router, cookie) = user_with(&app, "analyst", &["analysis.view"]).await;
    let (status, body) = get(
        &router,
        "/api/restock?min_copies=2&sort=velocity&desc=true",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["total"], 2,
        "Opt sold only one copy, Ancient is in stock"
    );
    assert_eq!(body["copies"], 5);
    assert_eq!(body["revenue"], Value::Null);
    let rows = body["rows"].as_array().unwrap();
    // Bolt: 3 copies in 7 days = 3/week; Shock: 2 in 14 days = 1/week.
    assert_eq!(rows[0]["name"], "Bolt");
    assert_eq!(rows[0]["copies_per_week"], 3.0);
    assert_eq!(rows[0]["days_to_sell_out"], 7);
    assert_eq!(rows[0]["last_price"], 1.0);
    assert_eq!(rows[0]["lot"], "L2");
    assert_eq!(rows[0]["realized_revenue"], Value::Null);
    assert!(rows[0]["cardmarket_url"]
        .as_str()
        .unwrap()
        .ends_with("idProduct=101"));
    assert_eq!(rows[1]["name"], "Shock");
    assert!(!body.to_string().contains("realized_revenue\":3"));

    // The order by revenue would give the money figures away.
    let (status, body) = get(
        &router,
        "/api/restock?min_copies=2&sort=revenue&desc=true",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "forbidden");

    let (_, body) = get(
        &router,
        "/api/restock?min_copies=1&sort=name&desc=false",
        &cookie,
    )
    .await;
    let names: Vec<&str> = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Bolt", "Opt", "Shock"]);
}

#[tokio::test]
async fn restock_shows_revenue_to_users_who_may_see_money() {
    let app = TestApp::new();
    inventory(&app).await;
    let (router, cookie) = user_with(&app, "owner", &["analysis.view", "financials.view"]).await;
    let (status, body) = get(
        &router,
        "/api/restock?min_copies=2&sort=revenue&desc=true",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["revenue"], 13.0);
    let rows = body["rows"].as_array().unwrap();
    assert_eq!(rows[0]["name"], "Shock");
    assert_eq!(rows[0]["realized_revenue"], 10.0);
    assert_eq!(rows[1]["realized_revenue"], 3.0);
}

async fn export(
    router: &Router,
    cookie: &str,
    query: &str,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let response = router
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/api/restock/export?{query}"),
            None,
            Some(cookie),
        ))
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn the_buy_list_has_revenue_only_for_users_who_may_see_money() {
    let app = TestApp::new();
    inventory(&app).await;
    let (router, analyst) = user_with(&app, "analyst", &["analysis.view"]).await;
    let (status, headers, csv) =
        export(&router, &analyst, "min_copies=2&sort=velocity&desc=true").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "text/csv; charset=utf-8");
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"restock_buy_list.csv\""
    );
    assert!(csv.starts_with("name,setCode,cn,condition,language,isFoil,rarity,lot,soldCopies,copiesPerWeek,daysToSellOut,lastPrice\n"));
    assert!(!csv.contains("realizedRevenue"));
    assert_eq!(csv.lines().count(), 3);
    assert!(csv.lines().nth(1).unwrap().starts_with("Bolt,"));

    let owner_cookie = {
        app.add_user("owner", &["analysis.view", "financials.view"])
            .await;
        login_ok(&router, "owner", PASSWORD).await
    };
    let (_, _, csv) = export(
        &router,
        &owner_cookie,
        "min_copies=2&sort=velocity&desc=true",
    )
    .await;
    assert!(csv.lines().next().unwrap().ends_with(",realizedRevenue"));
    assert!(csv.lines().nth(1).unwrap().ends_with(",3.00"));
}

#[tokio::test]
async fn invalid_restock_parameters_are_refused() {
    let app = TestApp::new();
    let (router, cookie) = user_with(&app, "analyst", &["analysis.view"]).await;
    for (query, code) in [
        ("min_copies=0&sort=velocity&desc=true", "invalid_params"),
        ("min_copies=1001&sort=velocity&desc=true", "invalid_params"),
        ("min_copies=2&sort=profit&desc=true", "invalid_request"),
        ("min_copies=2&sort=velocity", "invalid_request"),
        (
            "min_copies=2&sort=velocity&desc=true&x=1",
            "invalid_request",
        ),
    ] {
        let (status, body) = get(&router, &format!("/api/restock?{query}"), &cookie).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_eq!(body["error"]["code"], code, "{query}");
    }
    let (status, _, _) = export(&router, &cookie, "min_copies=0&sort=velocity&desc=true").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
