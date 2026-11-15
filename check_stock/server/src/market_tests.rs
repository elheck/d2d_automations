//! Tests of the market data service, and a fake tracker for the API tests.

use super::*;
use crate::db::Db;
use check_stock_core::inventory_db::{sync_inventory_on_conn, InventoryDb};
use check_stock_core::models::Card;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// Rising, falling and flat cards in the fake market.
pub(crate) const RISER: u64 = 101;
pub(crate) const FALLER: u64 = 102;
pub(crate) const FLAT: u64 = 103;

/// Trend `i` requested dates back (0 = today): the riser was cheaper
/// before, the faller dearer, the flat card stays at 5.
pub(crate) fn trend(id: u64, i: usize) -> f64 {
    let i = i as f64;
    match id {
        RISER => 10.0 / (1.0 + 0.01 * i),
        FALLER => 10.0 * (1.0 + 0.01 * i),
        _ => 5.0,
    }
}

struct Snapshots;

impl Respond for Snapshots {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or_default();
        let ids: Vec<u64> = serde_json::from_value(body["ids"].clone()).unwrap_or_default();
        let dates: Vec<String> = serde_json::from_value(body["dates"].clone()).unwrap_or_default();
        let mut rows = Vec::new();
        for (i, date) in dates.iter().enumerate() {
            for id in &ids {
                rows.push(
                    json!({ "id_product": id, "requested_date": date, "price_date": date,
                    "avg": null, "low": null, "trend": trend(*id, i), "avg1": null, "avg7": null,
                    "avg30": null, "avg_foil": null, "low_foil": null, "trend_foil": null,
                    "avg1_foil": null, "avg7_foil": null, "avg30_foil": null }),
                );
            }
        }
        ResponseTemplate::new(200).set_body_json(json!({ "success": true, "data": rows }))
    }
}

/// A fake inventory_sync with the three cards.
pub(crate) async fn fake_tracker() -> MockServer {
    let server = MockServer::start().await;
    let latest: Vec<Value> = [RISER, FALLER, FLAT]
        .into_iter()
        .map(|id| {
            json!({ "id_product": id, "price_date": "2026-11-15", "avg": null, "low": null,
                    "trend": trend(id, 0), "avg1": null, "avg7": null, "avg30": null,
                    "avg_foil": null, "low_foil": null, "trend_foil": null, "avg1_foil": null,
                    "avg7_foil": null, "avg30_foil": null })
        })
        .collect();
    Mock::given(method("POST"))
        .and(path("/api/latest-prices"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "success": true, "data": latest })),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/price-snapshots"))
        .respond_with(Snapshots)
        .mount(&server)
        .await;
    server
}

pub(crate) fn stock_card(id: u64, name: &str, qty: &str, price: &str) -> Card {
    Card {
        cardmarket_id: id.to_string(),
        name: name.to_string(),
        quantity: qty.to_string(),
        price: price.to_string(),
        set_code: "TST".to_string(),
        location: Some("A-1-1-1-L2-R".to_string()),
        listed_at: String::new(),
        ..Card::test_default()
    }
}

/// Three cards in stock since 2026-01-01.
pub(crate) async fn stock(db: &Db) {
    db.write(|conn| {
        sync_inventory_on_conn(
            conn,
            &[
                stock_card(RISER, "Riser", "2", "9.00"),
                stock_card(FALLER, "Faller", "1", "12.00"),
                stock_card(FLAT, "Flat", "4", "5.00"),
            ],
            DEFAULT_CATEGORY,
            "2026-01-01",
        )
        .map(|_| ())
    })
    .await
    .unwrap();
}

async fn db(dir: &std::path::Path) -> Db {
    let db = Db::open(&InventoryDb::in_dir(dir), 2).unwrap();
    stock(&db).await;
    db
}

async fn wait_until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn finished(status: JobStatus) -> bool {
    matches!(status, JobStatus::Finished { .. })
}

#[tokio::test]
async fn a_load_brings_prices_and_history_of_the_stock() {
    let tracker = fake_tracker().await;
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path()).await;
    let market = Arc::new(
        MarketService::new(Some(InventorySyncClient::new(&tracker.uri())))
            .with_cooldown(Duration::ZERO),
    );
    assert_eq!(market.start_load(db.clone(), false), Ok(true));
    wait_until(|| finished(market.status())).await;
    let data = market.loaded().unwrap();
    assert_eq!(data.cards.len(), 3);
    assert_eq!(data.products, 3);
    assert_eq!(data.version, 1);
    assert_eq!(data.latest[&RISER].trend, Some(10.0));
    assert_eq!(data.today, local_date(Utc::now().timestamp()));
    let change = data.snapshots.change(
        RISER,
        check_stock_core::api::inventory_sync::PriceField::Trend,
        false,
    );
    assert!(change.pct_7d.unwrap() > 0.0);

    // Today's data is there: a load without refresh does nothing.
    assert_eq!(market.start_load(db.clone(), false), Ok(false));
    assert_eq!(market.start_load(db.clone(), true), Ok(true));
    wait_until(|| market.loaded().is_some_and(|d| d.version == 2)).await;
}

#[tokio::test]
async fn without_a_tracker_or_stock_nothing_loads() {
    let dir = tempfile::tempdir().unwrap();
    let empty = Db::open(&InventoryDb::in_dir(dir.path()), 2).unwrap();
    let market = Arc::new(MarketService::new(None));
    assert_eq!(
        market.start_load(empty.clone(), false),
        Err(StartError::NotConfigured)
    );

    let tracker = fake_tracker().await;
    let market = Arc::new(MarketService::new(Some(InventorySyncClient::new(
        &tracker.uri(),
    ))));
    market.start_load(empty, false).unwrap();
    wait_until(|| matches!(market.status(), JobStatus::Failed { .. })).await;
    let JobStatus::Failed { error, .. } = market.status() else {
        unreachable!()
    };
    assert_eq!(error, "No cards in stock with a Cardmarket id.");
    assert!(market.loaded().is_none());
}

#[tokio::test]
async fn loads_are_not_repeated_too_soon_and_errors_hide_addresses() {
    let tracker = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .expect(2)
        .mount(&tracker)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path()).await;
    let market = Arc::new(
        MarketService::new(Some(InventorySyncClient::new(&tracker.uri())))
            .with_cooldown(Duration::from_secs(60)),
    );
    market.start_load(db.clone(), false).unwrap();
    assert_eq!(
        market.start_load(db.clone(), false),
        Err(StartError::Running)
    );
    wait_until(|| matches!(market.status(), JobStatus::Failed { .. })).await;
    let JobStatus::Failed { error, .. } = market.status() else {
        unreachable!()
    };
    // Retried once, then reported without the tracker's address.
    assert_eq!(error, "Price fetch failed.");
    assert!(matches!(
        market.start_load(db, true),
        Err(StartError::TooSoon { .. })
    ));
}

#[tokio::test]
async fn a_load_stopped_by_shutdown_stores_nothing() {
    let tracker = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "success": true, "data": [] }))
                .set_delay(Duration::from_millis(400)),
        )
        .mount(&tracker)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path()).await;
    let market = Arc::new(MarketService::new(Some(InventorySyncClient::new(
        &tracker.uri(),
    ))));
    market.start_load(db, false).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    market.shutdown();
    wait_until(|| matches!(market.status(), JobStatus::Cancelled { .. })).await;
    assert!(market.loaded().is_none());
}

#[tokio::test]
async fn the_history_is_fetched_in_portions() {
    let tracker = fake_tracker().await;
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&InventoryDb::in_dir(dir.path()), 2).unwrap();
    // More products than one portion holds.
    let cards: Vec<Card> = (0..HISTORY_CHUNK as u64 + 5)
        .map(|i| stock_card(10_000 + i, &format!("Card {i}"), "1", "1.00"))
        .collect();
    db.write(move |conn| {
        sync_inventory_on_conn(conn, &cards, DEFAULT_CATEGORY, "2026-01-01").map(|_| ())
    })
    .await
    .unwrap();
    let market = Arc::new(MarketService::new(Some(InventorySyncClient::new(
        &tracker.uri(),
    ))));
    market.start_load(db, false).unwrap();
    wait_until(|| finished(market.status())).await;
    let requests = tracker.received_requests().await.unwrap();
    let history: Vec<Value> = requests
        .iter()
        .filter(|r| r.url.path() == "/api/price-snapshots")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0]["ids"].as_array().unwrap().len(), HISTORY_CHUNK);
    assert_eq!(history[1]["ids"].as_array().unwrap().len(), 5);
    assert_eq!(market.loaded().unwrap().products, HISTORY_CHUNK + 5);
}
