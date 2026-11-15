use super::*;
use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use crate::market::tests::stock_card;
use axum::http::{Method, StatusCode};
use axum::Router;
use check_stock_core::inventory_db::{set_lot_cost_conn, sync_inventory_on_conn};
use serde_json::Value;

/// Lot L2: Bolt (3 × 1.00) and Shock (2 × 5.00) sold out, Opt (1 × 0.50)
/// sold, Ancient (4 × 2.00) still in stock; the lot cost 10.00.
async fn inventory(app: &TestApp) {
    app.state
        .db
        .write(|conn| {
            sync_inventory_on_conn(
                conn,
                &[
                    stock_card(101, "Bolt", "3", "1.00"),
                    stock_card(102, "Shock", "2", "5.00"),
                    stock_card(103, "Opt", "1", "0.50"),
                    stock_card(104, "Ancient", "4", "2.00"),
                ],
                DEFAULT_CATEGORY,
                "2026-01-01",
            )?;
            sync_inventory_on_conn(
                conn,
                &[stock_card(104, "Ancient", "4", "2.00")],
                DEFAULT_CATEGORY,
                "2026-01-08",
            )?;
            set_lot_cost_conn(conn, "L2", 10.0, "2026-01-08")
        })
        .await
        .unwrap();
}

async fn get_as(app: &TestApp, name: &str, keys: &[&str]) -> (StatusCode, Value) {
    app.add_user(name, keys).await;
    let router: Router = app.router();
    let cookie = login_ok(&router, name, PASSWORD).await;
    let (status, _, body) = send(
        &router,
        request(Method::GET, "/api/stock-analysis", None, Some(&cookie)),
    )
    .await;
    (status, body)
}

#[tokio::test]
async fn analysts_see_the_stock_without_money() {
    let app = TestApp::new();
    inventory(&app).await;
    let (status, body) = get_as(&app, "analyst", &["analysis.view"]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["money"], false);
    assert_eq!(body["in_stock_articles"], 1);
    assert_eq!(body["total_copies"], 4);
    assert_eq!(body["total_value"], Value::Null);
    assert_eq!(body["top_by_price"][0]["name"], "Ancient");
    assert_eq!(
        body["top_by_price"][0]["price"], 2.0,
        "card prices are not money totals"
    );
    let lot = &body["lots"][0];
    assert_eq!(lot["lot"], "L2");
    assert_eq!(lot["in_stock_copies"], 4);
    assert_eq!(lot["sold_copies"], 6);
    for money in [
        "in_stock_value",
        "sold_revenue",
        "bulk_revenue",
        "cost",
        "margin",
        "recouped",
        "cost_to_recoup",
    ] {
        assert_eq!(lot[money], Value::Null, "lot {money}");
    }
    assert_eq!(body["lot_totals"], Value::Null);
    assert_eq!(body["lots_sold_copies"], 6);
    assert_eq!(body["bulk_revenue"], Value::Null);
    for bucket in body["aging"].as_array().unwrap() {
        assert_eq!(bucket["value"], Value::Null);
    }
    if let Some(v) = body["velocity"].as_object() {
        assert_eq!(v["sold_revenue"], Value::Null);
        assert_eq!(v["revenue_per_week"], Value::Null);
    }
}

#[tokio::test]
async fn the_owner_sees_value_revenue_cost_and_margin() {
    let app = TestApp::new();
    inventory(&app).await;
    let (status, body) = get_as(&app, "owner", &["analysis.view", "financials.view"]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["money"], true);
    assert_eq!(body["total_value"], 8.0);
    let lot = &body["lots"][0];
    assert_eq!(lot["in_stock_value"], 8.0);
    assert_eq!(lot["sold_revenue"], 13.5);
    assert_eq!(lot["cost"], 10.0);
    assert!((lot["margin"].as_f64().unwrap() - 0.35).abs() < 1e-9);
    assert_eq!(lot["recouped"], true);
    assert_eq!(lot["cost_to_recoup"], 0.0);
    let totals = &body["lot_totals"];
    assert_eq!(totals["cost"], 10.0);
    assert_eq!(totals["sold_revenue"], 13.5);
    let aged: f64 = body["aging"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["value"].as_f64().unwrap())
        .sum();
    assert_eq!(aged, 8.0);
}

#[tokio::test]
async fn an_empty_database_is_fine() {
    let app = TestApp::new();
    let (status, body) = get_as(&app, "analyst", &["analysis.view"]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total_articles"], 0);
    assert_eq!(body["lots"].as_array().unwrap().len(), 0);
    assert_eq!(body["velocity"], Value::Null);
}
