//! Tests for the Scryfall card type lookup.

use super::*;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn card_json(set: &str, cn: &str, type_line: &str) -> serde_json::Value {
    serde_json::json!({
        "id": format!("{set}-{cn}"),
        "name": "Card",
        "set": set,
        "set_name": "Test Set",
        "collector_number": cn,
        "rarity": "common",
        "type_line": type_line
    })
}

fn printing(set: &str, cn: &str, id: Option<u64>) -> Printing {
    Printing {
        set_code: set.to_string(),
        cn: cn.to_string(),
        cardmarket_id: id,
    }
}

/// Runs the blocking lookup off the async test runtime.
async fn run(uri: String, printings: Vec<Printing>) -> (LookupOutcome, Vec<LookupProgress>) {
    tokio::task::spawn_blocking(move || {
        let cancel = AtomicBool::new(false);
        let mut steps = Vec::new();
        let out = lookup_types_from(
            &uri,
            &printings,
            &cancel,
            |p| steps.push(p),
            Duration::ZERO,
            Duration::ZERO,
        );
        (out, steps)
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn batch_lookup_resolves_front_face_types() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/cards/collection"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                card_json("mh3", "1", "Land — Gate"),
                card_json("mh3", "2", "Instant // Land")
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (out, steps) = run(
        server.uri(),
        vec![
            printing("MH3", "1", None),
            printing("MH3", "2", None),
            printing("mh3", "1", None), // duplicate, looked up once
        ],
    )
    .await;
    assert_eq!(
        out.found,
        vec![
            (
                "MH3".to_string(),
                "1".to_string(),
                "Land — Gate".to_string()
            ),
            ("MH3".to_string(), "2".to_string(), "Instant".to_string()),
        ]
    );
    assert!(out.missing.is_empty() && out.error.is_none());
    assert_eq!(steps, [LookupProgress::Batches { done: 1, total: 1 }]);
}

#[tokio::test]
async fn unknown_codes_fall_back_to_the_cardmarket_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/cards/collection"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": []
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/cards/cardmarket/777"))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json("mkm", "9", "Land")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/cards/cardmarket/888"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "status": 404, "code": "not_found", "details": "No card"
        })))
        .mount(&server)
        .await;

    let (out, steps) = run(
        server.uri(),
        vec![
            printing("XMKM", "9", Some(777)),
            printing("XMKM", "10", Some(888)),
            printing("XXX", "1", None),
        ],
    )
    .await;
    assert_eq!(
        out.found,
        vec![("XMKM".to_string(), "9".to_string(), "Land".to_string())]
    );
    let mut missing = out.missing.clone();
    missing.sort();
    assert_eq!(
        missing,
        vec![
            ("XMKM".to_string(), "10".to_string()),
            ("XXX".to_string(), "1".to_string())
        ]
    );
    assert!(out.error.is_none());
    assert_eq!(
        steps.last(),
        Some(&LookupProgress::Singles { done: 2, total: 2 })
    );
}

#[tokio::test]
async fn a_failing_server_stops_with_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/cards/collection"))
        .respond_with(ResponseTemplate::new(500))
        .expect(2) // first try + one retry
        .mount(&server)
        .await;
    let (out, _) = run(server.uri(), vec![printing("TST", "1", None)]).await;
    assert!(out.error.unwrap().contains("Scryfall lookup failed"));
    assert!(out.found.is_empty());
}

#[tokio::test]
async fn a_cancelled_lookup_sends_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let uri = server.uri();
    let out = tokio::task::spawn_blocking(move || {
        let cancel = AtomicBool::new(true);
        lookup_types_from(
            &uri,
            &[printing("TST", "1", None)],
            &cancel,
            |_| {},
            Duration::ZERO,
            Duration::ZERO,
        )
    })
    .await
    .unwrap();
    assert!(out.cancelled);
}

#[tokio::test]
async fn printings_without_set_or_number_are_skipped() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let (out, steps) = run(
        server.uri(),
        vec![printing("", "1", None), printing("TST", " ", None)],
    )
    .await;
    assert_eq!(out, LookupOutcome::default());
    assert!(steps.is_empty());
}

#[tokio::test]
async fn set_codes_scryfall_refuses_skip_the_batch() {
    // Regression: one Cardmarket-only code (7 characters) in a batch made
    // Scryfall reject the whole request and stopped the lookup.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/cards/collection"))
        .and(body_json(serde_json::json!({
            "identifiers": [{ "set": "mh3", "collector_number": "1" }]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [card_json("mh3", "1", "Land")]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/cards/cardmarket/555"))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json(
            "sld",
            "2001",
            "Creature — Elf",
        )))
        .expect(1)
        .mount(&server)
        .await;

    let (out, _) = run(
        server.uri(),
        vec![
            printing("MH3", "1", None),
            printing("SLDSV23", "7", Some(555)),
        ],
    )
    .await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.found.len(), 2);
    assert!(out.found.contains(&(
        "SLDSV23".to_string(),
        "7".to_string(),
        "Creature — Elf".to_string()
    )));
}

#[tokio::test]
async fn a_refused_batch_falls_back_to_single_lookups() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/cards/collection"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "status": 400,
            "code": "bad_request",
            "details": "A `set` identifier must be between 3-6 characters"
        })))
        .expect(1) // refused requests are not retried
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/cards/cardmarket/11"))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json("tst", "1", "Land")))
        .mount(&server)
        .await;

    let (out, steps) = run(
        server.uri(),
        vec![printing("TST", "1", Some(11)), printing("TST", "2", None)],
    )
    .await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(
        out.found,
        vec![("TST".to_string(), "1".to_string(), "Land".to_string())]
    );
    assert_eq!(out.missing, vec![("TST".to_string(), "2".to_string())]);
    assert_eq!(steps[0], LookupProgress::Batches { done: 1, total: 1 });
}
