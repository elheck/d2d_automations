use super::*;
use crate::api::inventory_sync::PriceField;
use crate::cache::deck_cache::INDEX_SCHEMA;
use crate::precon_ev::{build_rows, liquidity_weight, EvParams, PreconStatus, SalesStats};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const NOW: i64 = 1_792_000_000;
const ARMY: u64 = 774134;
const FAMILY: u64 = 774135;

fn product(id: u64, name: &str) -> Value {
    json!({ "id_product": id, "name": name, "category_name": "Magic Intropack",
            "id_expansion": 5660, "expansion_name": null })
}

fn catalog(extra: &[(u64, &str)]) -> Value {
    let mut data = vec![
        product(
            ARMY,
            "Commander: Bloomburrow: \"\"Animated Army\"\" Commander Deck",
        ),
        product(
            FAMILY,
            "Commander: Bloomburrow: \"\"Family Matters\"\" Commander Deck",
        ),
        json!({ "id_product": 1, "name": "Sol Ring Commander", "category_name": "Magic Single",
                "id_expansion": 1, "expansion_name": null }),
    ];
    data.extend(extra.iter().map(|(id, name)| product(*id, name)));
    json!({ "success": true, "data": data })
}

fn price_row(id: u64, trend: f64) -> Value {
    json!({ "id_product": id, "price_date": "2026-10-16", "avg": null, "low": null,
            "trend": trend, "avg1": null, "avg7": null, "avg30": null, "avg_foil": null,
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

fn deck_file(name: &str, ids: &[Option<&str>]) -> Value {
    let cards: Vec<Value> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let mut c = json!({ "name": format!("{name} card {i}"), "count": 1,
                                "isFoil": false, "setCode": "blc", "number": i.to_string() });
            if let Some(id) = id {
                c["identifiers"] = json!({ "mcmId": id });
            }
            c
        })
        .collect();
    json!({ "data": { "code": "BLC", "name": name, "releaseDate": "2024-08-02",
                      "commander": [], "mainBoard": cards } })
}

struct Servers {
    tracker: MockServer,
    mtgjson: MockServer,
}

async fn servers() -> Servers {
    Servers {
        tracker: MockServer::start().await,
        mtgjson: MockServer::start().await,
    }
}

async fn mount(server: &MockServer, verb: &str, p: &str, body: Value, times: Option<u64>) {
    let mock = Mock::given(method(verb))
        .and(path(p))
        .respond_with(ResponseTemplate::new(200).set_body_json(body));
    let mock = match times {
        Some(n) => mock.expect(n),
        None => mock,
    };
    mock.mount(server).await;
}

async fn mount_tracker(s: &Servers, times: Option<u64>) {
    mount(&s.tracker, "GET", "/api/search", catalog(&[]), times).await;
    let prices = json!({ "success": true, "data": [
        price_row(ARMY, 40.0), price_row(FAMILY, 30.0),
        price_row(101, 10.0), price_row(102, 0.2), price_row(201, 5.0)
    ]});
    mount(&s.tracker, "POST", "/api/latest-prices", prices, times).await;
}

fn snapshot_row(id: u64, date: &str, avg1: f64) -> Value {
    json!({ "id_product": id, "requested_date": date, "price_date": date, "avg": null,
            "low": null, "trend": 1.0, "avg1": avg1, "avg7": null, "avg30": null,
            "avg_foil": null, "low_foil": null, "trend_foil": null, "avg1_foil": null,
            "avg7_foil": null, "avg30_foil": null })
}

/// Daily snapshots for the sales window: card 101's 1-day average changes
/// every other day (7 sale days of 14), card 201's never changes.
async fn mount_sales(s: &Servers, times: Option<u64>) {
    let window = crate::precon_ev::SalesWindow::ending(local_date(NOW));
    let mut rows = vec![
        snapshot_row(101, &window.lookback, 9.0),
        snapshot_row(201, &window.lookback, 5.0),
    ];
    for (i, d) in window.daily.iter().enumerate() {
        rows.push(snapshot_row(101, d, 10.0 + (i / 2) as f64));
        rows.push(snapshot_row(201, d, 5.0));
    }
    let body = json!({ "success": true, "data": rows });
    mount(&s.tracker, "POST", "/api/price-snapshots", body, times).await;
}

type LoadResult = (Result<PreconData, String>, Vec<LoadEvent>);

async fn load(s: &Servers, cache_dir: PathBuf, force: bool, now: i64) -> LoadResult {
    let opts = LoadOptions {
        tracker: InventorySyncClient::new(&s.tracker.uri()),
        mtgjson: MtgjsonClient::new(&s.mtgjson.uri()),
        force_index_refresh: force,
        now_unix: now,
    };
    tokio::task::spawn_blocking(move || {
        let cache = DeckCache::with_dir(cache_dir);
        let events = Mutex::new(Vec::new());
        let result = load_precons(&opts, &cache, &|e| events.lock().unwrap().push(e));
        (result, events.into_inner().unwrap())
    })
    .await
    .unwrap()
}

fn last_decks_event(events: &[LoadEvent]) -> Option<(usize, usize)> {
    events.iter().rev().find_map(|e| match e {
        LoadEvent::Decks { done, total } => Some((*done, *total)),
        LoadEvent::Stage(_) | LoadEvent::Sales { .. } => None,
    })
}

#[tokio::test]
async fn load_end_to_end_then_served_from_cache() {
    let s = servers().await;
    let tmp = tempfile::tempdir().unwrap();
    mount_tracker(&s, Some(2)).await;
    // The ranking phase never asks for sales history.
    mount_sales(&s, Some(0)).await;
    mount(&s.mtgjson, "GET", "/SetList.json", set_list(), Some(1)).await;
    mount(&s.mtgjson, "GET", "/DeckList.json", deck_list(), Some(2)).await;
    mount(
        &s.mtgjson,
        "GET",
        "/decks/AnimatedArmy_BLC.json",
        deck_file("Animated Army", &[Some("101"), Some("102")]),
        Some(1),
    )
    .await;
    mount(
        &s.mtgjson,
        "GET",
        "/decks/FamilyMatters_BLC.json",
        deck_file("Family Matters", &[Some("201")]),
        Some(1),
    )
    .await;

    let (result, events) = load(&s, tmp.path().to_path_buf(), false, NOW).await;
    let data = result.unwrap();
    assert_eq!(data.catalog.len(), 2, "singles are filtered out");
    assert_eq!(data.decks.len(), 2);
    assert!(data.deck_errors.is_empty());
    assert!(!data.index_stale);
    assert_eq!(data.index_fetched_at_unix, NOW);
    assert_eq!(data.deck_list_version.as_deref(), Some("v2"));
    assert_eq!(last_decks_event(&events), Some((2, 2)));

    let rows = build_rows(
        &data,
        EvParams {
            field: PriceField::Trend,
            sealed_field: PriceField::Trend,
            cutoff: 0.5,
            foil_needs_sales: true,
        },
    );
    let army = rows.iter().find(|r| r.id_product == ARMY).unwrap();
    assert_eq!(army.status, PreconStatus::Ready { decks: 1 });
    assert!((army.ev.as_ref().unwrap().ev - 10.0).abs() < 1e-9);
    assert!((army.ev.as_ref().unwrap().top5 - 10.2).abs() < 1e-9);
    assert_eq!(army.sealed_price, Some(40.0));
    assert!(data.sales.is_none(), "sales are a separate phase");
    assert_eq!(army.ev.as_ref().unwrap().liquid, None);

    // Second load: decks and SetList come from the cache; catalog, DeckList
    // and prices are fetched live again (enforced by the `expect` counts).
    let (result, events) = load(&s, tmp.path().to_path_buf(), false, NOW + 60).await;
    let data = result.unwrap();
    assert_eq!(data.decks.len(), 2);
    assert_eq!(data.index_fetched_at_unix, NOW);
    assert_eq!(last_decks_event(&events), Some((2, 2)));
}

#[tokio::test]
async fn forced_refresh_and_new_products_rebuild_the_index() {
    let s = servers().await;
    let tmp = tempfile::tempdir().unwrap();
    // Third load sees a new catalog product → rebuild.
    Mock::given(method("GET"))
        .and(path("/api/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(catalog(&[])))
        .up_to_n_times(2)
        .mount(&s.tracker)
        .await;
    mount(
        &s.tracker,
        "GET",
        "/api/search",
        catalog(&[(
            916721,
            "Commander: Marvel: \"\"Strange Academy\"\" Commander Deck",
        )]),
        None,
    )
    .await;
    mount(
        &s.tracker,
        "POST",
        "/api/latest-prices",
        json!({ "success": true, "data": [] }),
        None,
    )
    .await;
    mount(&s.mtgjson, "GET", "/SetList.json", set_list(), Some(3)).await;
    mount(&s.mtgjson, "GET", "/DeckList.json", deck_list(), None).await;
    mount(
        &s.mtgjson,
        "GET",
        "/decks/AnimatedArmy_BLC.json",
        deck_file("Animated Army", &[Some("101")]),
        Some(1),
    )
    .await;
    mount(
        &s.mtgjson,
        "GET",
        "/decks/FamilyMatters_BLC.json",
        deck_file("Family Matters", &[Some("201")]),
        Some(1),
    )
    .await;

    let dir = tmp.path().to_path_buf();
    load(&s, dir.clone(), false, NOW).await.0.unwrap();
    let data = load(&s, dir.clone(), true, NOW + 60).await.0.unwrap();
    assert_eq!(data.index_fetched_at_unix, NOW + 60);
    let data = load(&s, dir.clone(), false, NOW + 120).await.0.unwrap();
    assert_eq!(data.index_fetched_at_unix, NOW + 120);
    let rows = build_rows(
        &data,
        EvParams {
            field: PriceField::Trend,
            sealed_field: PriceField::Trend,
            cutoff: 0.5,
            foil_needs_sales: true,
        },
    );
    let marvel = rows.iter().find(|r| r.id_product == 916721).unwrap();
    assert_eq!(marvel.status, PreconStatus::NotOnMtgjson);
}

#[tokio::test]
async fn incomplete_decks_are_refetched_and_failures_isolated() {
    let s = servers().await;
    let tmp = tempfile::tempdir().unwrap();
    mount_tracker(&s, None).await;
    mount(&s.mtgjson, "GET", "/SetList.json", set_list(), None).await;
    mount(&s.mtgjson, "GET", "/DeckList.json", deck_list(), None).await;
    mount(
        &s.mtgjson,
        "GET",
        "/decks/AnimatedArmy_BLC.json",
        deck_file("Animated Army", &[Some("101"), None]),
        Some(2),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/decks/FamilyMatters_BLC.json"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&s.mtgjson)
        .await;

    let dir = tmp.path().to_path_buf();
    for _ in 0..2 {
        let (result, events) = load(&s, dir.clone(), false, NOW).await;
        let data = result.unwrap();
        assert!(data.decks.contains_key("AnimatedArmy_BLC"));
        assert!(data.deck_errors["FamilyMatters_BLC"].contains("500"));
        assert_eq!(last_decks_event(&events), Some((2, 2)));
        let rows = build_rows(
            &data,
            EvParams {
                field: PriceField::Trend,
                sealed_field: PriceField::Trend,
                cutoff: 0.5,
                foil_needs_sales: true,
            },
        );
        let family = rows.iter().find(|r| r.id_product == FAMILY).unwrap();
        assert!(matches!(family.status, PreconStatus::DeckUnavailable(_)));
    }
}

#[tokio::test]
async fn mtgjson_outage_uses_stale_index_or_fails_without_one() {
    let s = servers().await;
    let tmp = tempfile::tempdir().unwrap();
    mount_tracker(&s, None).await;
    Mock::given(method("GET"))
        .and(path("/SetList.json"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&s.mtgjson)
        .await;
    mount(&s.mtgjson, "GET", "/DeckList.json", deck_list(), None).await;
    mount(
        &s.mtgjson,
        "GET",
        "/decks/AnimatedArmy_BLC.json",
        deck_file("Animated Army", &[Some("101")]),
        None,
    )
    .await;
    mount(
        &s.mtgjson,
        "GET",
        "/decks/FamilyMatters_BLC.json",
        deck_file("Family Matters", &[Some("201")]),
        None,
    )
    .await;

    // No cached index at all → error.
    let err = load(&s, tmp.path().to_path_buf(), false, NOW)
        .await
        .0
        .unwrap_err();
    assert!(err.contains("SetList"), "{err}");

    // An old cached index is used and flagged stale.
    let (_, sealed) =
        mtg_common::mtgjson::parse_set_list_sealed(set_list().to_string().as_bytes()).unwrap();
    let old = NOW - 30 * 24 * 60 * 60;
    DeckCache::with_dir(tmp.path().to_path_buf()).store_index(&CachedIndex {
        schema: INDEX_SCHEMA,
        fetched_at_unix: old,
        seen_catalog_ids: vec![ARMY, FAMILY],
        index: mtg_common::mtgjson::build_sealed_index(None, &sealed),
    });
    let data = load(&s, tmp.path().to_path_buf(), false, NOW)
        .await
        .0
        .unwrap();
    assert!(data.index_stale);
    assert_eq!(data.index_fetched_at_unix, old);
    assert_eq!(data.decks.len(), 2);
}

#[tokio::test]
async fn tracker_outage_fails_the_load() {
    let s = servers().await;
    let tmp = tempfile::tempdir().unwrap();
    Mock::given(method("GET"))
        .and(path("/api/search"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&s.tracker)
        .await;
    let err = load(&s, tmp.path().to_path_buf(), false, NOW)
        .await
        .0
        .unwrap_err();
    assert!(err.starts_with("Catalog fetch failed"), "{err}");
}

#[tokio::test]
async fn load_history_requests_cards_and_sealed_on_all_dates() {
    let tracker = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/price-snapshots"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "success": true, "data": [] })),
        )
        .expect(2)
        .mount(&tracker)
        .await;
    let uri = tracker.uri();
    let dates: Vec<String> = (1..=10).map(|d| format!("2026-01-{d:02}")).collect();
    let history = tokio::task::spawn_blocking(move || {
        load_history(
            &InventorySyncClient::new(&uri),
            ARMY,
            vec![101, 102, 101],
            dates,
        )
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(history.id_product, ARMY);
    assert_eq!(history.dates.len(), 10);
    let requests = tracker.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["ids"], json!([101, 102, ARMY]));
}

type SalesResult = (Result<crate::precon_ev::SalesMap, String>, Vec<LoadEvent>);

async fn run_sales(s: &Servers, dir: PathBuf, ids: Vec<u64>, cancelled: bool) -> SalesResult {
    let tracker = InventorySyncClient::new(&s.tracker.uri());
    tokio::task::spawn_blocking(move || {
        let events = Mutex::new(Vec::new());
        let cancel = AtomicBool::new(cancelled);
        let result = load_sales(
            &tracker,
            &SalesCache::with_dir(dir),
            &ids,
            local_date(NOW),
            &|e| events.lock().unwrap().push(e),
            &cancel,
        );
        (result, events.into_inner().unwrap())
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn sales_phase_counts_days_and_caches_them_for_the_day() {
    let s = servers().await;
    let tmp = tempfile::tempdir().unwrap();
    mount_tracker(&s, None).await;
    // One batch = 2 requests (16 dates), across both runs.
    mount_sales(&s, Some(2)).await;
    mount(&s.mtgjson, "GET", "/SetList.json", set_list(), None).await;
    mount(&s.mtgjson, "GET", "/DeckList.json", deck_list(), None).await;
    mount(
        &s.mtgjson,
        "GET",
        "/decks/AnimatedArmy_BLC.json",
        deck_file("Animated Army", &[Some("101"), Some("102")]),
        None,
    )
    .await;
    mount(
        &s.mtgjson,
        "GET",
        "/decks/FamilyMatters_BLC.json",
        deck_file("Family Matters", &[Some("201")]),
        None,
    )
    .await;
    let mut data = load(&s, tmp.path().join("decks"), false, NOW)
        .await
        .0
        .unwrap();

    // Only cards >= the price floor are checked (102 costs €0.20).
    let ids = crate::precon_ev::sales_candidates(&data.decks, &data.prices);
    assert_eq!(ids, vec![101, 201]);
    let (result, events) = run_sales(&s, tmp.path().join("sales"), ids.clone(), false).await;
    let sales = result.unwrap();
    assert_eq!(sales[&101].nonfoil_days, 7);
    assert_eq!(sales[&101].observed_days, 14);
    assert!(sales[&101].nonfoil_recent);
    assert_eq!(
        sales[&201],
        SalesStats {
            observed_days: 14,
            ..SalesStats::default()
        }
    );
    assert!(events.contains(&LoadEvent::Sales { done: 2, total: 2 }));

    // Same day again: served from the cache (the mock allows 2 requests).
    let (again, events) = run_sales(&s, tmp.path().join("sales"), ids, false).await;
    assert_eq!(again.unwrap(), sales);
    assert!(events.is_empty());

    data.sales = Some(sales);
    let rows = build_rows(
        &data,
        EvParams {
            field: PriceField::Trend,
            sealed_field: PriceField::Trend,
            cutoff: 0.5,
            foil_needs_sales: true,
        },
    );
    let army = rows.iter().find(|r| r.id_product == ARMY).unwrap();
    let liquid = army.ev.as_ref().unwrap().liquid.unwrap();
    assert!((liquid - 10.0 * liquidity_weight(7, 14)).abs() < 1e-9);
}

#[tokio::test]
async fn sales_batch_is_retried_once() {
    let s = servers().await;
    let tmp = tempfile::tempdir().unwrap();
    // First attempt fails, the retry succeeds (both of its requests).
    Mock::given(method("POST"))
        .and(path("/api/price-snapshots"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&s.tracker)
        .await;
    mount_sales(&s, Some(2)).await;
    let (result, _) = run_sales(&s, tmp.path().to_path_buf(), vec![101], false).await;
    assert_eq!(result.unwrap()[&101].nonfoil_days, 7);
}

#[tokio::test]
async fn sales_failure_and_cancel_return_errors() {
    let s = servers().await;
    let tmp = tempfile::tempdir().unwrap();
    Mock::given(method("POST"))
        .and(path("/api/price-snapshots"))
        .respond_with(ResponseTemplate::new(500))
        .expect(2) // first try + one retry, cancelled run sends nothing
        .mount(&s.tracker)
        .await;
    let (result, _) = run_sales(&s, tmp.path().to_path_buf(), vec![101], false).await;
    assert!(result
        .unwrap_err()
        .starts_with("Sales history fetch failed"));
    let (result, _) = run_sales(&s, tmp.path().to_path_buf(), vec![101], true).await;
    assert_eq!(result.unwrap_err(), "Sales history fetch cancelled");
}
