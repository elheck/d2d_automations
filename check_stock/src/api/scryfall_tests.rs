//! Tests for the Scryfall API client.

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::scryfall::{
    fetch_card_from, fetch_card_from_async, fetch_image, fetch_image_async, CardFace, ImageUris,
    ScryfallCard, ScryfallPrices,
};
use crate::error::ApiError;

/// Helper: creates a minimal ScryfallCard JSON value for mock responses.
fn scryfall_card_json(name: &str, set: &str, cn: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "test-uuid-123",
        "name": name,
        "set": set,
        "set_name": "Test Set",
        "collector_number": cn,
        "rarity": "common",
        "prices": { "eur": "1.50", "eur_foil": null, "usd": "2.00", "usd_foil": null },
        "image_uris": { "normal": "https://example.com/image.jpg" }
    })
}

fn scryfall_error_json(code: &str, details: &str) -> serde_json::Value {
    serde_json::json!({
        "status": 404,
        "code": code,
        "details": details
    })
}

// ── fetch_card_from ──────────────────────────────────────────────────

#[tokio::test]
async fn fetch_card_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/cards/lea/161"))
        .respond_with(ResponseTemplate::new(200).set_body_json(scryfall_card_json(
            "Lightning Bolt",
            "lea",
            "161",
        )))
        .mount(&mock_server)
        .await;

    let base_url = mock_server.uri();
    let result = tokio::task::spawn_blocking(move || fetch_card_from(&base_url, "LEA", "161"))
        .await
        .unwrap();

    let card = result.unwrap();
    assert_eq!(card.name, "Lightning Bolt");
    assert_eq!(card.set, "lea");
    assert_eq!(card.collector_number, "161");
}

#[tokio::test]
async fn fetch_card_lowercases_set_code() {
    let mock_server = MockServer::start().await;

    // The mock expects lowercase "m10"
    Mock::given(method("GET"))
        .and(path("/cards/m10/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(scryfall_card_json(
            "Test Card",
            "m10",
            "42",
        )))
        .mount(&mock_server)
        .await;

    let base_url = mock_server.uri();
    // Pass uppercase "M10" — should be lowercased
    let result = tokio::task::spawn_blocking(move || fetch_card_from(&base_url, "M10", "42"))
        .await
        .unwrap();

    assert!(result.is_ok(), "Should match the lowercase path");
}

#[tokio::test]
async fn fetch_card_404_returns_api_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/cards/xxx/999"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(scryfall_error_json(
                "not_found",
                "No card found with the given set and collector number",
            )),
        )
        .mount(&mock_server)
        .await;

    let base_url = mock_server.uri();
    let result = tokio::task::spawn_blocking(move || fetch_card_from(&base_url, "xxx", "999"))
        .await
        .unwrap();

    match result {
        Err(ApiError::ApiResponse { code, details }) => {
            assert_eq!(code, "not_found");
            assert!(details.contains("No card found"));
        }
        other => panic!("Expected ApiError::ApiResponse, got: {other:?}"),
    }
}

#[tokio::test]
async fn fetch_card_deserializes_prices() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/cards/lea/161"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "uuid",
            "name": "Bolt",
            "set": "lea",
            "set_name": "Alpha",
            "collector_number": "161",
            "rarity": "common",
            "prices": { "eur": "50.00", "eur_foil": "100.00", "usd": "55.00", "usd_foil": null },
            "image_uris": { "normal": "https://example.com/img.jpg" }
        })))
        .mount(&mock_server)
        .await;

    let base_url = mock_server.uri();
    let result = tokio::task::spawn_blocking(move || fetch_card_from(&base_url, "lea", "161"))
        .await
        .unwrap();

    let card = result.unwrap();
    assert_eq!(card.prices.eur.as_deref(), Some("50.00"));
    assert_eq!(card.prices.eur_foil.as_deref(), Some("100.00"));
    assert_eq!(card.prices.usd.as_deref(), Some("55.00"));
    assert!(card.prices.usd_foil.is_none());
}

#[tokio::test]
async fn fetch_card_deserializes_cardmarket_id() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/cards/lea/161"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "uuid",
            "name": "Bolt",
            "set": "lea",
            "set_name": "Alpha",
            "collector_number": "161",
            "rarity": "common",
            "prices": {},
            "cardmarket_id": 12345
        })))
        .mount(&mock_server)
        .await;

    let base_url = mock_server.uri();
    let result = tokio::task::spawn_blocking(move || fetch_card_from(&base_url, "lea", "161"))
        .await
        .unwrap();

    assert_eq!(result.unwrap().cardmarket_id, Some(12345));
}

// ── fetch_image ──────────────────────────────────────────────────────

#[tokio::test]
async fn fetch_image_success() {
    let mock_server = MockServer::start().await;

    let image_bytes = vec![0x89, 0x50, 0x4E, 0x47]; // PNG header bytes

    Mock::given(method("GET"))
        .and(path("/image.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(image_bytes.clone()))
        .mount(&mock_server)
        .await;

    let url = format!("{}/image.png", mock_server.uri());
    let result = tokio::task::spawn_blocking(move || fetch_image(&url))
        .await
        .unwrap();

    assert_eq!(result.unwrap(), image_bytes);
}

#[tokio::test]
async fn fetch_image_404_returns_http_status() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/missing.png"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let url = format!("{}/missing.png", mock_server.uri());
    let result = tokio::task::spawn_blocking(move || fetch_image(&url))
        .await
        .unwrap();

    match result {
        Err(ApiError::HttpStatus(status)) => {
            assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
        }
        other => panic!("Expected ApiError::HttpStatus(404), got: {other:?}"),
    }
}

// ── ScryfallCard::image_url ──────────────────────────────────────────

#[test]
fn image_url_from_image_uris() {
    let card = ScryfallCard {
        id: "id".to_string(),
        name: "Test".to_string(),
        set: "tst".to_string(),
        set_name: "Test Set".to_string(),
        collector_number: "1".to_string(),
        rarity: "common".to_string(),
        prices: ScryfallPrices::default(),
        image_uris: Some(ImageUris {
            small: None,
            normal: Some("https://example.com/normal.jpg".to_string()),
            large: None,
            png: None,
            art_crop: None,
            border_crop: None,
        }),
        card_faces: None,
        cardmarket_id: None,
        mana_cost: None,
        type_line: None,
        oracle_text: None,
    };

    assert_eq!(card.image_url(), Some("https://example.com/normal.jpg"));
}

#[test]
fn image_url_from_card_faces() {
    let card = ScryfallCard {
        id: "id".to_string(),
        name: "DFC".to_string(),
        set: "tst".to_string(),
        set_name: "Test Set".to_string(),
        collector_number: "1".to_string(),
        rarity: "rare".to_string(),
        prices: ScryfallPrices::default(),
        image_uris: None, // No top-level image
        card_faces: Some(vec![
            CardFace {
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
            },
            CardFace {
                name: "Back".to_string(),
                image_uris: Some(ImageUris {
                    small: None,
                    normal: Some("https://example.com/back.jpg".to_string()),
                    large: None,
                    png: None,
                    art_crop: None,
                    border_crop: None,
                }),
                mana_cost: None,
                type_line: None,
                oracle_text: None,
            },
        ]),
        cardmarket_id: None,
        mana_cost: None,
        type_line: None,
        oracle_text: None,
    };

    // Should return the front face image
    assert_eq!(card.image_url(), Some("https://example.com/front.jpg"));
}

#[test]
fn image_url_none_when_both_missing() {
    let card = ScryfallCard {
        id: "id".to_string(),
        name: "No Image".to_string(),
        set: "tst".to_string(),
        set_name: "Test Set".to_string(),
        collector_number: "1".to_string(),
        rarity: "common".to_string(),
        prices: ScryfallPrices::default(),
        image_uris: None,
        card_faces: None,
        cardmarket_id: None,
        mana_cost: None,
        type_line: None,
        oracle_text: None,
    };

    assert_eq!(card.image_url(), None);
}

// ── Async fetch_card_from_async ──────────────────────────────────────

#[tokio::test]
async fn fetch_card_async_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/cards/lea/161"))
        .respond_with(ResponseTemplate::new(200).set_body_json(scryfall_card_json(
            "Lightning Bolt",
            "lea",
            "161",
        )))
        .mount(&mock_server)
        .await;

    let card = fetch_card_from_async(&mock_server.uri(), "LEA", "161")
        .await
        .unwrap();

    assert_eq!(card.name, "Lightning Bolt");
    assert_eq!(card.set, "lea");
    assert_eq!(card.collector_number, "161");
}

#[tokio::test]
async fn fetch_card_async_lowercases_set_code() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/cards/m10/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(scryfall_card_json(
            "Test Card",
            "m10",
            "42",
        )))
        .mount(&mock_server)
        .await;

    // Pass uppercase "M10" — should be lowercased in the URL
    let result = fetch_card_from_async(&mock_server.uri(), "M10", "42").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn fetch_card_async_404_returns_api_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/cards/xxx/999"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(scryfall_error_json(
                "not_found",
                "No card found with the given set and collector number",
            )),
        )
        .mount(&mock_server)
        .await;

    let result = fetch_card_from_async(&mock_server.uri(), "xxx", "999").await;

    match result {
        Err(ApiError::ApiResponse { code, details }) => {
            assert_eq!(code, "not_found");
            assert!(details.contains("No card found"));
        }
        other => panic!("Expected ApiError::ApiResponse, got: {other:?}"),
    }
}

#[tokio::test]
async fn fetch_card_async_deserializes_prices() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/cards/lea/161"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "uuid",
            "name": "Bolt",
            "set": "lea",
            "set_name": "Alpha",
            "collector_number": "161",
            "rarity": "common",
            "prices": { "eur": "50.00", "eur_foil": "100.00", "usd": "55.00", "usd_foil": null },
            "image_uris": { "normal": "https://example.com/img.jpg" }
        })))
        .mount(&mock_server)
        .await;

    let card = fetch_card_from_async(&mock_server.uri(), "lea", "161")
        .await
        .unwrap();

    assert_eq!(card.prices.eur.as_deref(), Some("50.00"));
    assert_eq!(card.prices.eur_foil.as_deref(), Some("100.00"));
    assert!(card.prices.usd_foil.is_none());
}

#[tokio::test]
async fn fetch_card_async_deserializes_cardmarket_id() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/cards/lea/161"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "uuid",
            "name": "Bolt",
            "set": "lea",
            "set_name": "Alpha",
            "collector_number": "161",
            "rarity": "common",
            "prices": {},
            "cardmarket_id": 99999
        })))
        .mount(&mock_server)
        .await;

    let card = fetch_card_from_async(&mock_server.uri(), "lea", "161")
        .await
        .unwrap();

    assert_eq!(card.cardmarket_id, Some(99999));
}

// ── Async fetch_image_async ───────────────────────────────────────────

#[tokio::test]
async fn fetch_image_async_success() {
    let mock_server = MockServer::start().await;

    let image_bytes = vec![0x89, 0x50, 0x4E, 0x47]; // PNG header bytes

    Mock::given(method("GET"))
        .and(path("/image.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(image_bytes.clone()))
        .mount(&mock_server)
        .await;

    let url = format!("{}/image.png", mock_server.uri());
    let result = fetch_image_async(&url).await.unwrap();

    assert_eq!(result, image_bytes);
}

#[tokio::test]
async fn fetch_image_async_404_returns_http_status() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/missing.png"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let url = format!("{}/missing.png", mock_server.uri());
    let result = fetch_image_async(&url).await;

    match result {
        Err(ApiError::HttpStatus(status)) => {
            assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
        }
        other => panic!("Expected ApiError::HttpStatus(404), got: {other:?}"),
    }
}

#[tokio::test]
async fn fetch_image_async_returns_full_bytes() {
    let mock_server = MockServer::start().await;

    // A 16-byte payload representing an arbitrary binary blob
    let payload: Vec<u8> = (0u8..16).collect();

    Mock::given(method("GET"))
        .and(path("/img.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.clone()))
        .mount(&mock_server)
        .await;

    let url = format!("{}/img.jpg", mock_server.uri());
    let result = fetch_image_async(&url).await.unwrap();

    assert_eq!(result, payload);
    assert_eq!(result.len(), 16);
}
