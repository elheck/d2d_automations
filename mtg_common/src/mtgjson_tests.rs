//! Tests for the MTGJSON client and precon parsing.

use super::*;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn key(set: &str, name: &str) -> DeckKey {
    DeckKey {
        set_code: set.to_string(),
        name: name.to_string(),
    }
}

fn sealed(uuid: &str, mcm: Option<u64>, decks: &[(&str, &str)]) -> SealedProduct {
    SealedProduct {
        uuid: uuid.to_string(),
        name: format!("Product {uuid}"),
        set_code: "BLC".to_string(),
        subtype: Some("commander".to_string()),
        mcm_id: mcm,
        decks: decks.iter().map(|(s, n)| key(s, n)).collect(),
        children: Vec::new(),
    }
}

fn bundle(uuid: &str, mcm: Option<u64>, children: &[(&str, u32)]) -> SealedProduct {
    SealedProduct {
        children: children.iter().map(|(u, c)| (u.to_string(), *c)).collect(),
        ..sealed(uuid, mcm, &[])
    }
}

fn entry(code: &str, name: &str, file: &str) -> DeckListEntry {
    DeckListEntry {
        code: code.to_string(),
        file_name: file.to_string(),
        name: name.to_string(),
        release_date: Some("2024-08-02".to_string()),
    }
}

fn card_json(name: &str, count: u64, foil: bool, mcm: Option<&str>) -> serde_json::Value {
    let mut c = json!({
        "name": name, "count": count, "isFoil": foil, "setCode": "blc", "number": "1",
        "legalities": { "commander": "Legal" }, "foreignData": []
    });
    if let Some(id) = mcm {
        c["identifiers"] = json!({ "mcmId": id, "scryfallId": "x" });
    }
    c
}

fn deck_json() -> serde_json::Value {
    json!({
        "meta": { "version": "5.3.0" },
        "data": {
            "code": "BLC", "name": "Animated Army", "releaseDate": "2024-08-02",
            "type": "Commander Deck",
            "commander": [card_json("Bello, Bard of the Brambles", 1, true, Some("778264"))],
            "mainBoard": [
                card_json("Sol Ring", 1, false, Some("778300")),
                card_json("Forest", 10, false, Some("778400")),
            ],
            "sideBoard": [card_json("Extra", 1, false, None)],
            "displayCommander": [card_json("Bello Oversized", 1, true, Some("778999"))],
            "tokens": [card_json("Beast Token", 1, false, Some("1"))],
            "planes": [], "schemes": []
        }
    })
}

// ── File name whitelist / URLs / ids ────────────────────────────────────────

#[test]
fn safe_file_names_accepted() {
    assert!(is_safe_deck_file_name("AnimatedArmy_BLC"));
    assert!(is_safe_deck_file_name("20WaysToWin_SLD"));
    assert!(is_safe_deck_file_name(&"a".repeat(MAX_DECK_FILE_NAME_LEN)));
}

#[test]
fn unsafe_file_names_rejected() {
    for bad in [
        "",
        "../x",
        "a/b",
        "a\\b",
        "a.b",
        "a b",
        "%2e%2e",
        "ArtistSeriesVéroniqueMeignaud_SLD",
        "a\0b",
        "x?y",
    ] {
        assert!(!is_safe_deck_file_name(bad), "{bad:?} should be rejected");
    }
    assert!(!is_safe_deck_file_name(
        &"a".repeat(MAX_DECK_FILE_NAME_LEN + 1)
    ));
}

#[test]
fn deck_url_trims_and_validates() {
    assert_eq!(
        deck_url("https://mtgjson.com/api/v5/", "AnimatedArmy_BLC").unwrap(),
        "https://mtgjson.com/api/v5/decks/AnimatedArmy_BLC.json"
    );
    assert!(deck_url("https://mtgjson.com/api/v5", "../SetList").is_err());
}

#[test]
fn parse_mcm_id_rules() {
    assert_eq!(parse_mcm_id(" 774134 "), Some(774134));
    assert_eq!(parse_mcm_id("0"), None);
    assert_eq!(parse_mcm_id("abc"), None);
    assert_eq!(parse_mcm_id("-1"), None);
    assert_eq!(mcm_id_from_value(&json!(42)), Some(42));
    assert_eq!(mcm_id_from_value(&json!(null)), None);
}

// ── DeckList / SetList parsing ──────────────────────────────────────────────

#[test]
fn parse_deck_list_reads_version_and_tolerates_nulls() {
    let body = json!({
        "meta": { "date": "2026-10-16", "version": "5.3.0+20261016" },
        "data": [
            { "code": "BLC", "fileName": "AnimatedArmy_BLC", "name": "Animated Army",
              "releaseDate": "2024-08-02", "type": "Commander Deck", "extra": 1 },
            { "code": "SLD", "fileName": "X_SLD", "name": "X", "releaseDate": null, "type": "Secret Lair Drop" }
        ]
    });
    let (version, decks) = parse_deck_list(body.to_string().as_bytes()).unwrap();
    assert_eq!(version.as_deref(), Some("5.3.0+20261016"));
    assert_eq!(decks.len(), 2);
    assert_eq!(decks[0].file_name, "AnimatedArmy_BLC");
    assert_eq!(decks[1].release_date, None);
}

#[test]
fn parse_set_list_sealed_flattens_and_tolerates_missing_fields() {
    let body = json!({
        "meta": { "version": "5.3.0" },
        "data": [
            { "code": "blc", "name": "Bloomburrow Commander", "sealedProduct": [
                { "uuid": "u1", "name": "Animated Army", "category": "deck", "subtype": "commander",
                  "identifiers": { "mcmId": "774134" },
                  "contents": { "deck": [{ "name": "Animated Army", "set": "blc" }],
                                "sealed": [{ "uuid": "sample", "count": 1, "name": "Sample" }],
                                "other": [{ "name": "Deck box" }] } },
                { "uuid": "u2", "name": "No ids", "subtype": "commander" },
                { "uuid": "u3", "name": "Numeric id", "identifiers": { "mcmId": 99 } },
                { "name": "No uuid" }
            ]},
            { "code": "OLD", "sealedProduct": null },
            { "code": "NEW" }
        ]
    });
    let (version, products) = parse_set_list_sealed(body.to_string().as_bytes()).unwrap();
    assert_eq!(version.as_deref(), Some("5.3.0"));
    assert_eq!(products.len(), 3);
    let p = &products[0];
    assert_eq!(p.set_code, "BLC");
    assert_eq!(p.mcm_id, Some(774134));
    assert_eq!(p.decks, vec![key("BLC", "Animated Army")]);
    assert_eq!(p.children, vec![("sample".to_string(), 1)]);
    assert_eq!(products[1].mcm_id, None);
    assert_eq!(products[2].mcm_id, Some(99));
}

// ── Sealed index ────────────────────────────────────────────────────────────

#[test]
fn index_keeps_commander_products_with_ids() {
    let mut other = sealed("u3", Some(3), &[("BLB", "Starter")]);
    other.subtype = Some("planeswalker".to_string());
    let products = vec![
        sealed("u1", Some(1), &[("BLC", "Animated Army")]),
        sealed("u2", None, &[("BLC", "Family Matters")]),
        other,
        sealed("u4", Some(4), &[]),
    ];
    let index = build_sealed_index(Some("v".into()), &products);
    assert_eq!(index.mtgjson_version.as_deref(), Some("v"));
    assert_eq!(index.products.len(), 1);
    assert_eq!(
        index.products[&1].deck_keys,
        vec![key("BLC", "Animated Army")]
    );
}

#[test]
fn index_own_decks_take_precedence_over_sample_pack_children() {
    let mut deck = sealed("u1", Some(1), &[("BLC", "Animated Army")]);
    deck.children = vec![("sample".to_string(), 1)];
    let sample = sealed("sample", None, &[("BLB", "Should Not Appear")]);
    let index = build_sealed_index(None, &[deck, sample]);
    assert_eq!(
        index.products[&1].deck_keys,
        vec![key("BLC", "Animated Army")]
    );
}

#[test]
fn index_expands_bundles_with_counts() {
    let a = sealed("a", Some(10), &[("FDC", "Calling All Angels")]);
    let b = sealed("b", Some(11), &[("FDC", "Keen Engineering")]);
    let set = bundle("set", Some(20), &[("a", 1), ("b", 2), ("missing", 1)]);
    let index = build_sealed_index(None, &[a, b, set]);
    assert_eq!(
        index.products[&20].deck_keys,
        vec![
            key("FDC", "Calling All Angels"),
            key("FDC", "Keen Engineering"),
            key("FDC", "Keen Engineering"),
        ]
    );
    assert_eq!(index.products.len(), 3);
}

#[test]
fn index_terminates_on_cycles_and_respects_depth() {
    let a = bundle("a", Some(1), &[("b", 1)]);
    let b = bundle("b", Some(2), &[("a", 1)]);
    let index = build_sealed_index(None, &[a, b]);
    assert!(index.products.is_empty());

    // Decks three levels down are beyond MAX_SEALED_DEPTH.
    let top = bundle("top", Some(1), &[("mid", 1)]);
    let mid = bundle("mid", None, &[("low", 1)]);
    let low = bundle("low", None, &[("deck", 1)]);
    let deck = sealed("deck", None, &[("X", "Deep")]);
    let index = build_sealed_index(None, &[top, mid, low, deck]);
    assert!(index.products.is_empty());
}

#[test]
fn index_drops_products_with_too_many_decks() {
    let deck = sealed("d", None, &[("X", "Deck")]);
    let huge = bundle("huge", Some(1), &[("d", 100)]);
    let index = build_sealed_index(None, &[deck, huge]);
    assert!(index.products.is_empty());
}

#[test]
fn index_duplicate_ids_prefer_more_decks_then_first_seen() {
    let first = sealed("u1", Some(5), &[("A", "One")]);
    let second = sealed("u2", Some(5), &[("A", "Two")]);
    let bigger = sealed("u3", Some(6), &[("A", "One")]);
    let biggest = sealed("u4", Some(6), &[("A", "One"), ("A", "Two")]);
    let index = build_sealed_index(None, &[first, second, bigger, biggest]);
    assert_eq!(index.products[&5].deck_keys, vec![key("A", "One")]);
    assert_eq!(index.products[&6].deck_keys.len(), 2);
}

#[test]
fn sealed_index_serde_round_trip() {
    let index = build_sealed_index(
        Some("v".into()),
        &[sealed("u1", Some(774134), &[("BLC", "Animated Army")])],
    );
    let json = serde_json::to_string(&index).unwrap();
    let back: SealedIndex = serde_json::from_str(&json).unwrap();
    assert_eq!(back, index);
}

// ── Deck resolution ─────────────────────────────────────────────────────────

#[test]
fn resolve_matches_case_insensitively() {
    let index = build_sealed_index(None, &[sealed("u1", Some(1), &[("blc", "ANIMATED ARMY")])]);
    let res = resolve_decks(&index, &[entry("BLC", "Animated Army", "AnimatedArmy_BLC")]);
    match &res[&1] {
        DeckResolution::Ready(refs) => {
            assert_eq!(refs.len(), 1);
            assert_eq!(refs[0].file_name, "AnimatedArmy_BLC");
            assert_eq!(refs[0].release_date.as_deref(), Some("2024-08-02"));
        }
        other => panic!("expected Ready, got {other:?}"),
    }
}

#[test]
fn resolve_reports_missing_and_unsafe_decks() {
    let index = build_sealed_index(
        None,
        &[
            sealed("u1", Some(1), &[("TRC", "We Are the Borg")]),
            sealed("u2", Some(2), &[("SLD", "Bad")]),
        ],
    );
    let res = resolve_decks(&index, &[entry("SLD", "Bad", "../../etc_SLD")]);
    assert_eq!(
        res[&1],
        DeckResolution::Pending(vec!["We Are the Borg".to_string()])
    );
    assert_eq!(
        res[&2],
        DeckResolution::Pending(vec!["Bad (unsupported deck file name)".to_string()])
    );
}

// ── Deck parsing ────────────────────────────────────────────────────────────

#[test]
fn parse_deck_maps_boards_and_drops_tokens() {
    let deck = parse_deck(deck_json().to_string().as_bytes()).unwrap();
    assert_eq!(deck.code, "BLC");
    assert_eq!(deck.name, "Animated Army");
    assert_eq!(deck.release_date.as_deref(), Some("2024-08-02"));
    assert_eq!(deck.cards.len(), 5);
    assert!(deck.cards.iter().all(|c| c.name != "Beast Token"));

    let bello = &deck.cards[0];
    assert_eq!(bello.board, DeckBoard::Commander);
    assert!(bello.is_foil);
    assert_eq!(bello.mcm_id, Some(778264));
    assert_eq!(bello.set_code, "BLC");

    assert_eq!(deck.cards[2].count, 10);
    assert_eq!(deck.cards[3].board, DeckBoard::Side);
    assert_eq!(deck.cards[3].mcm_id, None);
    assert_eq!(deck.cards[4].board, DeckBoard::Display);
}

#[test]
fn precon_deck_helpers() {
    let deck = parse_deck(deck_json().to_string().as_bytes()).unwrap();
    // 1 commander + 1 + 10 main + 1 side; display excluded.
    assert_eq!(deck.total_copies(), 13);
    assert!(!deck.is_complete());
    let ids: Vec<u64> = deck.card_ids().collect();
    assert_eq!(ids, vec![778264, 778300, 778400, 778999]);

    let json = serde_json::to_string(&deck).unwrap();
    let back: PreconDeck = serde_json::from_str(&json).unwrap();
    assert_eq!(back, deck);
}

#[test]
fn parse_deck_count_rules() {
    let mut body = deck_json();
    body["data"]["mainBoard"] = json!([
        { "name": "No count", "identifiers": { "mcmId": "5" } },
        { "name": "Zero", "count": 0 },
        { "name": "", "count": 1 }
    ]);
    let deck = parse_deck(body.to_string().as_bytes()).unwrap();
    let names: Vec<&str> = deck.cards.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"No count"));
    assert!(!names.contains(&"Zero"));
    assert_eq!(
        deck.cards
            .iter()
            .find(|c| c.name == "No count")
            .unwrap()
            .count,
        1
    );

    body["data"]["mainBoard"] = json!([{ "name": "Absurd", "count": 100000 }]);
    assert!(matches!(
        parse_deck(body.to_string().as_bytes()),
        Err(MtgError::Api { .. })
    ));
}

#[test]
fn parse_deck_invalid_json_is_parse_error() {
    assert!(matches!(parse_deck(b"{not json"), Err(MtgError::Parse(_))));
}

#[test]
fn validate_rejects_out_of_bounds_decks() {
    let mut deck = parse_deck(deck_json().to_string().as_bytes()).unwrap();
    assert!(deck.validate().is_ok());
    deck.cards[0].count = 0;
    assert!(deck.validate().is_err());
    deck.cards[0].count = 1;
    deck.cards[0].mcm_id = Some(0);
    assert!(deck.validate().is_err());
    deck.cards[0].mcm_id = Some(1);
    deck.cards[1].count = 250;
    deck.cards[2].count = 250;
    deck.cards[3].count = 250;
    deck.cards[4].count = 250;
    assert!(deck.validate().is_err());
}

// ── HTTP (wiremock) ─────────────────────────────────────────────────────────

#[test]
fn client_trims_base_url_and_defaults_to_mtgjson() {
    assert_eq!(MtgjsonClient::new("http://x//").base_url(), "http://x");
    assert_eq!(MtgjsonClient::default().base_url(), MTGJSON_API);
    assert!(MtgjsonClient::default().https_only());
    assert!(!MtgjsonClient::new("http://127.0.0.1:1").https_only());
}

#[tokio::test]
async fn deck_fetch_parses_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/decks/AnimatedArmy_BLC.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(deck_json()))
        .expect(1)
        .mount(&server)
        .await;
    let deck = MtgjsonClient::new(&server.uri())
        .deck("AnimatedArmy_BLC")
        .await
        .unwrap();
    assert_eq!(deck.name, "Animated Army");
}

#[tokio::test]
async fn deck_fetch_maps_http_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let err = MtgjsonClient::new(&server.uri())
        .deck("Missing_X")
        .await
        .unwrap_err();
    assert!(matches!(err, MtgError::HttpStatus(s) if s.as_u16() == 404));
}

#[tokio::test]
async fn unsafe_deck_name_sends_no_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let err = MtgjsonClient::new(&server.uri())
        .deck("../SetList")
        .await
        .unwrap_err();
    assert!(matches!(err, MtgError::Api { .. }));
}

#[tokio::test]
async fn capped_read_enforces_limit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/body"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 100]))
        .mount(&server)
        .await;
    let client = MtgjsonClient::new(&server.uri());
    let url = format!("{}/body", server.uri());
    assert_eq!(
        client
            .get_capped(&url, HTTP_TIMEOUT, 100)
            .await
            .unwrap()
            .len(),
        100
    );
    assert!(matches!(
        client.get_capped(&url, HTTP_TIMEOUT, 99).await,
        Err(MtgError::Api { .. })
    ));
}

#[tokio::test]
async fn sealed_index_and_deck_list_fetch() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/SetList.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "meta": { "version": "v1" },
            "data": [{ "code": "BLC", "sealedProduct": [
                { "uuid": "u1", "name": "Animated Army", "subtype": "commander",
                  "identifiers": { "mcmId": "774134" },
                  "contents": { "deck": [{ "name": "Animated Army", "set": "blc" }] } }
            ]}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/DeckList.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "meta": { "version": "v1" },
            "data": [{ "code": "BLC", "fileName": "AnimatedArmy_BLC", "name": "Animated Army" }]
        })))
        .mount(&server)
        .await;
    let client = MtgjsonClient::new(&server.uri());
    let index = client.sealed_index().await.unwrap();
    assert_eq!(index.mtgjson_version.as_deref(), Some("v1"));
    let (_, decks) = client.deck_list().await.unwrap();
    assert!(matches!(
        &resolve_decks(&index, &decks)[&774134],
        DeckResolution::Ready(refs) if refs[0].file_name == "AnimatedArmy_BLC"
    ));
}

#[cfg(feature = "blocking")]
#[tokio::test]
async fn blocking_variants_work_and_enforce_cap() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/decks/AnimatedArmy_BLC.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(deck_json()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/body"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 100]))
        .mount(&server)
        .await;
    let uri = server.uri();
    let (deck, at_cap, over_cap, unsafe_name) = tokio::task::spawn_blocking(move || {
        let client = MtgjsonClient::new(&uri);
        let url = format!("{uri}/body");
        (
            client.deck_blocking("AnimatedArmy_BLC"),
            client.get_capped_blocking(&url, HTTP_TIMEOUT, 100),
            client.get_capped_blocking(&url, HTTP_TIMEOUT, 99),
            client.deck_blocking("a.b"),
        )
    })
    .await
    .unwrap();
    assert_eq!(deck.unwrap().cards.len(), 5);
    assert_eq!(at_cap.unwrap().len(), 100);
    assert!(matches!(over_cap, Err(MtgError::Api { .. })));
    assert!(matches!(unsafe_name, Err(MtgError::Api { .. })));
}
