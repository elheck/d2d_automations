//! Tests for Scryfall API client
//!
//! Note: Some tests require network access and are marked with #[ignore]

use crate::scryfall::ScryfallCard;

#[test]
fn test_scryfall_card_image_url_direct() {
    let card_json = r#"{
        "name": "Black Lotus",
        "image_uris": {
            "normal": "https://example.com/normal.jpg",
            "large": "https://example.com/large.jpg"
        }
    }"#;

    let card: ScryfallCard = serde_json::from_str(card_json).unwrap();
    assert_eq!(card.image_url(), Some("https://example.com/normal.jpg"));
}

#[test]
fn test_scryfall_card_image_url_double_faced() {
    let card_json = r#"{
        "name": "Delver of Secrets",
        "card_faces": [
            {
                "name": "Delver of Secrets",
                "image_uris": {
                    "normal": "https://example.com/front.jpg"
                }
            },
            {
                "name": "Insectile Aberration",
                "image_uris": {
                    "normal": "https://example.com/back.jpg"
                }
            }
        ]
    }"#;

    let card: ScryfallCard = serde_json::from_str(card_json).unwrap();
    // Should return front face image
    assert_eq!(card.image_url(), Some("https://example.com/front.jpg"));
}

#[test]
fn test_scryfall_card_image_url_none() {
    let card_json = r#"{
        "name": "Test Card"
    }"#;

    let card: ScryfallCard = serde_json::from_str(card_json).unwrap();
    assert_eq!(card.image_url(), None);
}

#[test]
fn test_scryfall_card_deserialize_minimal() {
    let card_json = r#"{
        "name": "Test Card"
    }"#;

    let card: ScryfallCard = serde_json::from_str(card_json).unwrap();
    assert_eq!(card.name, "Test Card");
    assert!(card.image_uris.is_none());
    assert!(card.card_faces.is_none());
}

// Integration tests (require network access)
#[tokio::test]
#[ignore] // Run with: cargo test -- --ignored
async fn test_fetch_card_by_name_integration() {
    use crate::scryfall::fetch_card_by_name;

    let result = fetch_card_by_name("Lightning Bolt").await;
    assert!(result.is_ok());

    let card = result.unwrap();
    assert!(card.name.to_lowercase().contains("lightning"));
    assert!(card.image_url().is_some());
}

#[tokio::test]
#[ignore] // Run with: cargo test -- --ignored
async fn test_fetch_card_not_found_integration() {
    use crate::scryfall::fetch_card_by_name;

    let result = fetch_card_by_name("ThisCardDoesNotExistXYZ123").await;
    assert!(result.is_err());
}
