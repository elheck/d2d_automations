//! Tests of the precon service, and a fake tracker + MTGJSON for the API
//! tests (`pub(crate)` fixtures).

use super::*;
use serde_json::{json, Value};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

pub(crate) const ARMY: u64 = 774134;
pub(crate) const FAMILY: u64 = 774135;

fn product(id: u64, name: &str) -> Value {
    json!({ "id_product": id, "name": name, "category_name": "Magic Intropack",
            "id_expansion": 5660, "expansion_name": "Bloomburrow Commander" })
}

fn catalog() -> Value {
    json!({ "success": true, "data": [
        product(ARMY, "Commander: Bloomburrow: \"\"Animated Army\"\" Commander Deck"),
        product(FAMILY, "Commander: Bloomburrow: \"\"Family Matters\"\" Commander Deck"),
        json!({ "id_product": 1, "name": "Sol Ring Commander", "category_name": "Magic Single",
                "id_expansion": 1, "expansion_name": null }),
    ]})
}

/// Trend price of each product: sealed 40 / 30, cards 10, 0.20 and 5.
pub(crate) fn trend(id: u64) -> f64 {
    match id {
        ARMY => 40.0,
        FAMILY => 30.0,
        101 => 10.0,
        102 => 0.2,
        201 => 5.0,
        _ => 1.0,
    }
}

fn price_row(id: u64) -> Value {
    json!({ "id_product": id, "price_date": "2026-11-14", "avg": null, "low": trend(id) * 0.9,
            "trend": trend(id), "avg1": null, "avg7": null, "avg30": null, "avg_foil": null,
            "low_foil": null, "trend_foil": null, "avg1_foil": null, "avg7_foil": null,
            "avg30_foil": null })
}

fn set_list() -> Value {
    let deck = |uuid: &str, mcm: u64, name: &str| {
        json!({ "uuid": uuid, "name": name, "category": "deck", "subtype": "commander",
                "identifiers": { "mcmId": mcm.to_string() },
                "contents": { "deck": [{ "name": name, "set": "blc" }] } })
    };
    json!({ "meta": { "version": "v1" }, "data": [{ "code": "BLC", "sealedProduct": [
        deck("u1", ARMY, "Animated Army"),
        deck("u2", FAMILY, "Family Matters"),
    ]}]})
}

fn deck_list() -> Value {
    json!({ "meta": { "version": "v2" }, "data": [
        { "code": "BLC", "fileName": "AnimatedArmy_BLC", "name": "Animated Army",
          "releaseDate": "2024-08-02" },
        { "code": "BLC", "fileName": "FamilyMatters_BLC", "name": "Family Matters",
          "releaseDate": "2024-08-02" }
    ]})
}

fn deck_file(name: &str, ids: &[u64]) -> Value {
    let cards: Vec<Value> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            json!({ "name": format!("{name} card {i}"), "count": 1, "isFoil": false,
                    "setCode": "blc", "number": i.to_string(),
                    "identifiers": { "mcmId": id.to_string() } })
        })
        .collect();
    json!({ "data": { "code": "BLC", "name": name, "releaseDate": "2024-08-02",
                      "commander": [], "mainBoard": cards } })
}

/// Answers `/api/price-snapshots` for whatever ids and dates are asked:
/// trend as above, a 1-day average that changes every day (a sale a day).
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
                    "avg": null, "low": null, "trend": trend(*id), "avg1": 10.0 + i as f64,
                    "avg7": null, "avg30": null, "avg_foil": null, "low_foil": null,
                    "trend_foil": null, "avg1_foil": null, "avg7_foil": null,
                    "avg30_foil": null }),
                );
            }
        }
        ResponseTemplate::new(200).set_body_json(json!({ "success": true, "data": rows }))
    }
}

/// A fake inventory_sync and MTGJSON with two Bloomburrow precons.
pub(crate) struct FakeServices {
    pub tracker: MockServer,
    pub mtgjson: MockServer,
}

impl FakeServices {
    pub async fn start() -> Self {
        let fake = Self {
            tracker: MockServer::start().await,
            mtgjson: MockServer::start().await,
        };
        fake.mount_tracker().await;
        fake.mount_mtgjson().await;
        fake
    }

    pub async fn mount_tracker(&self) {
        get(&self.tracker, "/api/search", catalog()).await;
        let prices: Vec<Value> = [ARMY, FAMILY, 101, 102, 201]
            .into_iter()
            .map(price_row)
            .collect();
        Mock::given(method("POST"))
            .and(path("/api/latest-prices"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "success": true, "data": prices })),
            )
            .mount(&self.tracker)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/price-snapshots"))
            .respond_with(Snapshots)
            .mount(&self.tracker)
            .await;
    }

    pub async fn mount_mtgjson(&self) {
        get(&self.mtgjson, "/SetList.json", set_list()).await;
        get(&self.mtgjson, "/DeckList.json", deck_list()).await;
        get(
            &self.mtgjson,
            "/decks/AnimatedArmy_BLC.json",
            deck_file("Animated Army", &[101, 102]),
        )
        .await;
        get(
            &self.mtgjson,
            "/decks/FamilyMatters_BLC.json",
            deck_file("Family Matters", &[201]),
        )
        .await;
    }

    /// A precon service using these fakes, caches in `cache_root`, no cooldown.
    pub fn service(&self, cache_root: &Path) -> PreconService {
        PreconService::new(
            cache_root,
            Some(InventorySyncClient::new(&self.tracker.uri())),
            MtgjsonClient::new(&self.mtgjson.uri()),
        )
        .with_cooldown(Duration::ZERO)
    }
}

async fn get(server: &MockServer, p: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// Waits (up to 10 s) until `done` holds.
pub(crate) async fn wait_for(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn snapshot_requests(tracker: &MockServer) -> usize {
    tracker
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.url.path() == "/api/price-snapshots")
        .count()
}

fn finished(status: JobStatus) -> bool {
    matches!(status, JobStatus::Finished { .. })
}

#[test]
fn errors_for_people_leave_out_addresses() {
    assert_eq!(
        public_message("Catalog fetch failed: error sending request for url (http://cardscanner.local:3000/api/search)"),
        "Catalog fetch failed."
    );
    assert_eq!(
        public_message("Sales history fetch cancelled"),
        "Sales history fetch cancelled."
    );
    assert_eq!(public_message(""), "The load failed.");
}

#[tokio::test]
async fn without_a_tracker_nothing_loads() {
    let dir = tempfile::tempdir().unwrap();
    let service = Arc::new(PreconService::new(
        dir.path(),
        None,
        MtgjsonClient::new("http://127.0.0.1:9"),
    ));
    assert!(!service.is_configured());
    assert_eq!(service.start_load(false), Err(StartError::NotConfigured));
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
    assert!(matches!(
        service.history(ARMY, today).await,
        Err(HistoryError::NotConfigured)
    ));
}

#[tokio::test]
async fn a_load_brings_the_ranking_then_the_sales_history() {
    let fake = FakeServices::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Arc::new(fake.service(dir.path()));
    assert!(service.loaded().is_none());
    service.start_load(false).unwrap();
    wait_for(|| finished(service.sales_status())).await;
    assert!(finished(service.load_status()));

    let loaded = service.loaded().unwrap();
    assert_eq!(loaded.data.catalog.len(), 2);
    assert_eq!(loaded.data.decks.len(), 2);
    // Version 1 = ranking data, 2 = with the sales history.
    assert_eq!(loaded.version, 2);
    let sales = loaded.data.sales.as_ref().expect("sales history");
    assert!(sales.get(&101).is_some_and(|s| s.nonfoil_days > 0));
    // The decklists are cached in the desktop app's layout.
    assert!(DeckCache::in_root(dir.path())
        .get("AnimatedArmy_BLC")
        .is_some());
}

#[tokio::test]
async fn one_load_at_a_time_and_not_too_often() {
    let fake = FakeServices::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Arc::new(
        fake.service(dir.path())
            .with_cooldown(Duration::from_secs(60)),
    );
    service.start_load(false).unwrap();
    assert_eq!(service.start_load(false), Err(StartError::Running));
    wait_for(|| finished(service.load_status())).await;
    match service.start_load(true) {
        Err(StartError::TooSoon { wait }) => assert!(wait <= Duration::from_secs(60)),
        other => panic!("expected TooSoon, got {other:?}"),
    }
}

#[tokio::test]
async fn the_cooldown_counts_from_the_end_of_a_slow_load() {
    let fake = FakeServices {
        tracker: MockServer::start().await,
        mtgjson: MockServer::start().await,
    };
    fake.mount_tracker().await;
    // The load takes longer than the cooldown.
    Mock::given(method("GET"))
        .and(path("/SetList.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(set_list())
                .set_delay(Duration::from_millis(600)),
        )
        .mount(&fake.mtgjson)
        .await;
    fake.mount_mtgjson().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Arc::new(
        fake.service(dir.path())
            .with_cooldown(Duration::from_millis(400)),
    );
    service.start_load(false).unwrap();
    wait_for(|| finished(service.load_status())).await;
    assert!(matches!(
        service.start_load(false),
        Err(StartError::TooSoon { .. })
    ));
    tokio::time::sleep(Duration::from_millis(450)).await;
    wait_for(|| !service.sales_status().is_running()).await;
    assert_eq!(service.start_load(false), Ok(()));
    wait_for(|| finished(service.load_status())).await;
}

#[tokio::test]
async fn sales_of_an_older_load_are_not_attached() {
    let dir = tempfile::tempdir().unwrap();
    let service = PreconService::new(dir.path(), None, MtgjsonClient::new("http://127.0.0.1:9"));
    let v1 = service.store(PreconData::default());
    let v2 = service.store(PreconData::default());
    assert_eq!((v1, v2), (1, 2));
    service.attach_sales(v1, Ok(SalesMap::new()));
    let current = service.loaded().unwrap();
    assert_eq!(current.version, 2);
    assert!(current.data.sales.is_none());
    service.attach_sales(v2, Err("Sales history fetch failed.".to_string()));
    let current = service.loaded().unwrap();
    assert_eq!(current.version, 3);
    assert_eq!(
        current.data.sales_error.as_deref(),
        Some("Sales history fetch failed.")
    );
}

#[tokio::test]
async fn failures_are_reported_without_addresses() {
    let fake = FakeServices {
        tracker: MockServer::start().await,
        mtgjson: MockServer::start().await,
    };
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&fake.tracker)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let service = Arc::new(fake.service(dir.path()));
    service.start_load(false).unwrap();
    wait_for(|| matches!(service.load_status(), JobStatus::Failed { .. })).await;
    let JobStatus::Failed { error, .. } = service.load_status() else {
        unreachable!()
    };
    assert_eq!(error, "Catalog fetch failed.");
    assert!(service.loaded().is_none());
}

#[tokio::test]
async fn a_load_stopped_by_shutdown_stores_nothing() {
    let fake = FakeServices {
        tracker: MockServer::start().await,
        mtgjson: MockServer::start().await,
    };
    fake.mount_tracker().await;
    // A slow MTGJSON keeps the load busy while the server shuts down.
    Mock::given(method("GET"))
        .and(path("/SetList.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(set_list())
                .set_delay(Duration::from_millis(500)),
        )
        .mount(&fake.mtgjson)
        .await;
    get(&fake.mtgjson, "/DeckList.json", deck_list()).await;
    get(
        &fake.mtgjson,
        "/decks/AnimatedArmy_BLC.json",
        deck_file("Animated Army", &[101, 102]),
    )
    .await;
    get(
        &fake.mtgjson,
        "/decks/FamilyMatters_BLC.json",
        deck_file("Family Matters", &[201]),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let service = Arc::new(fake.service(dir.path()));
    service.start_load(false).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    service.shutdown();
    wait_for(|| matches!(service.load_status(), JobStatus::Cancelled { .. })).await;
    assert!(service.loaded().is_none());
}

#[tokio::test]
async fn histories_are_fetched_once_per_load() {
    let fake = FakeServices::start().await;
    let dir = tempfile::tempdir().unwrap();
    let service = Arc::new(fake.service(dir.path()));
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
    assert!(matches!(
        service.history(ARMY, today).await,
        Err(HistoryError::NotLoaded)
    ));
    service.start_load(false).unwrap();
    wait_for(|| finished(service.sales_status())).await;

    let before = snapshot_requests(&fake.tracker).await;
    let (_, history) = service.history(ARMY, today).await.unwrap();
    assert_eq!(history.id_product, ARMY);
    assert_eq!(history.dates.last().map(String::as_str), Some("2026-11-15"));
    let after_first = snapshot_requests(&fake.tracker).await;
    assert!(after_first > before);
    service.history(ARMY, today).await.unwrap();
    assert_eq!(
        snapshot_requests(&fake.tracker).await,
        after_first,
        "served from memory"
    );

    assert!(matches!(
        service.history(999, today).await,
        Err(HistoryError::NoDecklist)
    ));

    // A new load starts over.
    service.start_load(false).unwrap();
    wait_for(|| {
        finished(service.sales_status()) && service.loaded().is_some_and(|l| l.version == 4)
    })
    .await;
    service.history(ARMY, today).await.unwrap();
    assert!(snapshot_requests(&fake.tracker).await > after_first);
}
