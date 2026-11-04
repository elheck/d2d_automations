//! Tests for scryfall.

use super::*;

#[test]
fn image_url_from_direct_uris() {
    let uris = ImageUris {
        small: None,
        normal: Some("https://example.com/normal.jpg".to_string()),
        large: None,
        png: None,
        art_crop: None,
        border_crop: None,
    };
    assert_eq!(
        image_url(Some(&uris), None),
        Some("https://example.com/normal.jpg")
    );
}

#[test]
fn image_url_from_card_face() {
    let faces = vec![CardFace {
        name: "Front".to_string(),
        image_uris: Some(ImageUris {
            small: None,
            normal: Some("https://example.com/front.jpg".to_string()),
            large: None,
            png: None,
            art_crop: None,
            border_crop: None,
        }),
        mana_cost: None,
        type_line: None,
        oracle_text: None,
    }];
    assert_eq!(
        image_url(None, Some(&faces)),
        Some("https://example.com/front.jpg")
    );
}

#[test]
fn image_url_prefers_direct_over_faces() {
    let uris = ImageUris {
        small: None,
        normal: Some("https://example.com/direct.jpg".to_string()),
        large: None,
        png: None,
        art_crop: None,
        border_crop: None,
    };
    let faces = vec![CardFace {
        name: "Front".to_string(),
        image_uris: Some(ImageUris {
            small: None,
            normal: Some("https://example.com/face.jpg".to_string()),
            large: None,
            png: None,
            art_crop: None,
            border_crop: None,
        }),
        mana_cost: None,
        type_line: None,
        oracle_text: None,
    }];
    assert_eq!(
        image_url(Some(&uris), Some(&faces)),
        Some("https://example.com/direct.jpg")
    );
}

#[test]
fn image_url_none_when_empty() {
    assert_eq!(image_url(None, None), None);
}

#[test]
fn image_uris_deserializes_with_missing_fields() {
    let json = r#"{"normal": "https://example.com/img.jpg"}"#;
    let uris: ImageUris = serde_json::from_str(json).unwrap();
    assert_eq!(uris.normal.as_deref(), Some("https://example.com/img.jpg"));
    assert!(uris.small.is_none());
    assert!(uris.png.is_none());
}

// ── front_type_line ──────────────────────────────────────────────────

fn card_with_types(type_line: Option<&str>, faces: Option<Vec<Option<&str>>>) -> ScryfallCard {
    ScryfallCard {
        id: "id".to_string(),
        name: "Card".to_string(),
        set: "tst".to_string(),
        set_name: "Test".to_string(),
        collector_number: "1".to_string(),
        rarity: "common".to_string(),
        prices: ScryfallPrices::default(),
        image_uris: None,
        card_faces: faces.map(|faces| {
            faces
                .into_iter()
                .map(|t| CardFace {
                    name: "Face".to_string(),
                    image_uris: None,
                    mana_cost: None,
                    type_line: t.map(str::to_string),
                    oracle_text: None,
                })
                .collect()
        }),
        cardmarket_id: None,
        mana_cost: None,
        type_line: type_line.map(str::to_string),
        oracle_text: None,
        purchase_uris: None,
    }
}

#[test]
fn front_type_line_of_a_single_faced_card() {
    let card = card_with_types(Some("Land"), None);
    assert_eq!(card.front_type_line(), Some("Land"));
}

#[test]
fn front_type_line_takes_the_front_of_a_double_faced_card() {
    // A modal DFC with a land back is not a land.
    let card = card_with_types(Some("Instant // Land"), None);
    assert_eq!(card.front_type_line(), Some("Instant"));
}

#[test]
fn front_type_line_falls_back_to_the_first_face() {
    // Reversible cards carry type lines only on their faces.
    let card = card_with_types(None, Some(vec![Some("Creature — Elf"), Some("Land")]));
    assert_eq!(card.front_type_line(), Some("Creature — Elf"));
}

#[test]
fn front_type_line_none_when_missing() {
    assert_eq!(card_with_types(None, None).front_type_line(), None);
    assert_eq!(card_with_types(Some("  "), None).front_type_line(), None);
}

// ── fetch_collection ─────────────────────────────────────────────────

use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn collection_card_json(set: &str, cn: &str, type_line: &str) -> serde_json::Value {
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

#[test]
fn collection_identifier_lowercases_the_set() {
    let id = CollectionIdentifier::new(" MH3 ", " 12a ");
    assert_eq!(id.set, "mh3");
    assert_eq!(id.collector_number, "12a");
}

#[tokio::test]
async fn fetch_collection_posts_identifiers_and_returns_found_cards() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/cards/collection"))
        .and(header("Accept", "application/json"))
        .and(body_json(serde_json::json!({
            "identifiers": [
                { "set": "mh3", "collector_number": "1" },
                { "set": "mh3", "collector_number": "999" }
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "object": "list",
            "not_found": [{ "set": "mh3", "collector_number": "999" }],
            "data": [collection_card_json("mh3", "1", "Land")]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ids = [
        CollectionIdentifier::new("MH3", "1"),
        CollectionIdentifier::new("MH3", "999"),
    ];
    let cards = fetch_collection_from(&server.uri(), &ids).await.unwrap();
    assert_eq!(cards.len(), 1, "unresolved identifiers are simply absent");
    assert_eq!(cards[0].collector_number, "1");
    assert_eq!(cards[0].front_type_line(), Some("Land"));
}

#[tokio::test]
async fn fetch_collection_with_no_identifiers_sends_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let cards = fetch_collection_from(&server.uri(), &[]).await.unwrap();
    assert!(cards.is_empty());
}

#[tokio::test]
async fn fetch_collection_rejects_oversized_batches_before_sending() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let ids: Vec<_> = (0..=COLLECTION_MAX_IDENTIFIERS)
        .map(|i| CollectionIdentifier::new("tst", &i.to_string()))
        .collect();
    let err = fetch_collection_from(&server.uri(), &ids)
        .await
        .unwrap_err();
    assert!(matches!(err, MtgError::Api { ref code, .. } if code == "too_many_identifiers"));
}

#[test]
fn collection_identifiers_follow_scryfall_rules() {
    for (set, cn) in [
        ("mh3", "1"),
        ("PLST", "MH2-12"),
        ("sld", "123a"),
        ("pmei", "7★"),
    ] {
        assert!(
            CollectionIdentifier::new(set, cn).is_accepted(),
            "{set}/{cn}"
        );
    }
    for (set, cn) in [
        ("SLDSV23", "1"), // Cardmarket-only code, 7 characters
        ("xy", "1"),
        ("m-21", "1"),
        ("tst", ""),
        ("tst", "T 1/2"),
        ("tst", "1/2"),
    ] {
        assert!(
            !CollectionIdentifier::new(set, cn).is_accepted(),
            "{set}/{cn}"
        );
    }
}

#[tokio::test]
async fn fetch_collection_rejects_invalid_identifiers_before_sending() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let ids = [
        CollectionIdentifier::new("mh3", "1"),
        CollectionIdentifier::new("SLDSV23", "1"),
    ];
    let err = fetch_collection_from(&server.uri(), &ids)
        .await
        .unwrap_err();
    assert!(matches!(err, MtgError::Api { ref code, .. } if code == "invalid_identifier"));
}

#[tokio::test]
async fn fetch_collection_maps_scryfall_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/cards/collection"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "status": 400,
            "code": "bad_request",
            "details": "Malformed identifiers"
        })))
        .mount(&server)
        .await;
    let err = fetch_collection_from(&server.uri(), &[CollectionIdentifier::new("tst", "1")])
        .await
        .unwrap_err();
    assert!(matches!(err, MtgError::Api { ref code, .. } if code == "bad_request"));
}
