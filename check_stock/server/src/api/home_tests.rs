use crate::api::tests::{login_ok, request, send, TestApp, PASSWORD};
use crate::market::tests::stock_card;
use axum::http::{Method, StatusCode};
use check_stock_core::category::DEFAULT_CATEGORY;
use check_stock_core::inventory_db::sync_inventory_on_conn;
use serde_json::Value;

/// Bolt (3 × 1.00) on 2026-01-01; 2 sold by 2026-01-05, Shock new.
async fn history(app: &TestApp) {
    app.state
        .db
        .write(|conn| {
            sync_inventory_on_conn(
                conn,
                &[stock_card(101, "Bolt", "3", "1.00")],
                DEFAULT_CATEGORY,
                "2026-01-01",
            )?;
            sync_inventory_on_conn(
                conn,
                &[
                    stock_card(101, "Bolt", "1", "1.00"),
                    stock_card(102, "Shock", "1", "5.00"),
                ],
                DEFAULT_CATEGORY,
                "2026-01-05",
            )
        })
        .await
        .unwrap();
}

/// A user whose previous visit was 2026-01-02.
async fn visitor(app: &TestApp, name: &str, keys: &[&str]) -> String {
    let id = app.add_user(name, keys).await;
    app.state
        .db
        .write(move |conn| {
            conn.execute(
                "INSERT INTO srv_user_visits (user_id, last_visit) VALUES (?1, '2026-01-02')",
                [id],
            )
        })
        .await
        .unwrap();
    login_ok(&app.router(), name, PASSWORD).await
}

async fn digest(app: &TestApp, cookie: &str) -> (StatusCode, Value) {
    let (status, _, body) = send(
        &app.router(),
        request(Method::POST, "/api/digest", None, Some(cookie)),
    )
    .await;
    (status, body)
}

#[tokio::test]
async fn the_owner_sees_sales_and_revenue_since_the_last_visit() {
    let app = TestApp::new();
    history(&app).await;
    let cookie = visitor(&app, "owner", &["analysis.view", "financials.view"]).await;
    let (status, body) = digest(&app, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["since"], "2026-01-02");
    assert_eq!(body["last_import"], "2026-01-05");
    assert_eq!(body["sold_copies"], 2);
    assert_eq!(body["new_listings"], 1);
    assert_eq!(body["sold_revenue"], 2.0);
    assert_eq!(body["bulk_revenue"], 0.0);
    // The same day again: same baseline.
    let (_, again) = digest(&app, &cookie).await;
    assert_eq!(again["since"], "2026-01-02");
}

#[tokio::test]
async fn analysts_see_no_revenue_and_others_no_sales() {
    let app = TestApp::new();
    history(&app).await;
    let analyst = visitor(&app, "analyst", &["analysis.view"]).await;
    let (_, body) = digest(&app, &analyst).await;
    assert_eq!(body["sold_copies"], 2);
    assert_eq!(body["sold_revenue"], Value::Null);
    assert_eq!(body["bulk_revenue"], Value::Null);

    let money_only = visitor(&app, "bookkeeper", &["financials.view"]).await;
    let (_, body) = digest(&app, &money_only).await;
    assert_eq!(
        body["sold_revenue"],
        Value::Null,
        "money needs the reports too"
    );

    let helper = visitor(&app, "helper", &["search.use"]).await;
    let (_, body) = digest(&app, &helper).await;
    for hidden in [
        "sold_copies",
        "new_listings",
        "bulk_lots",
        "restock_candidates",
        "sold_revenue",
        "bulk_revenue",
    ] {
        assert_eq!(body[hidden], Value::Null, "{hidden}");
    }
    assert_eq!(body["last_import"], "2026-01-05");
}

#[tokio::test]
async fn the_first_visit_has_no_baseline() {
    let app = TestApp::new();
    app.add_user("new", &["analysis.view"]).await;
    let cookie = login_ok(&app.router(), "new", PASSWORD).await;
    let (status, body) = digest(&app, &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["since"], Value::Null);
    assert_eq!(body["sold_copies"], 0);
}
