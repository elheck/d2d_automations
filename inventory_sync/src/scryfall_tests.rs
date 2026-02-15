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

#[test]
fn test_scryfall_card_with_metadata() {
    let card_json = r#"{
        "name": "Lightning Bolt",
        "set_name": "Ravnica: Clue Edition",
        "type_line": "Instant",
        "mana_cost": "{R}",
        "rarity": "uncommon",
        "oracle_text": "Lightning Bolt deals 3 damage to any target.",
        "purchase_uris": {
            "cardmarket": "https://www.cardmarket.com/en/Magic/Products/Singles/Ravnica-Clue-Edition/Lightning-Bolt",
            "tcgplayer": "https://www.tcgplayer.com/product/534658"
        },
        "image_uris": {
            "normal": "https://example.com/normal.jpg"
        }
    }"#;

    let card: ScryfallCard = serde_json::from_str(card_json).unwrap();
    assert_eq!(card.set_name.as_deref(), Some("Ravnica: Clue Edition"));
    assert_eq!(card.type_line.as_deref(), Some("Instant"));
    assert_eq!(card.mana_cost.as_deref(), Some("{R}"));
    assert_eq!(card.rarity.as_deref(), Some("uncommon"));
    assert_eq!(
        card.oracle_text.as_deref(),
        Some("Lightning Bolt deals 3 damage to any target.")
    );
    assert!(card.purchase_uris.is_some());
    let uris = card.purchase_uris.as_ref().unwrap();
    assert!(uris.cardmarket.as_ref().unwrap().contains("cardmarket.com"));
}

#[test]
fn test_card_info_extraction() {
    let card_json = r#"{
        "name": "Lightning Bolt",
        "set_name": "Ravnica: Clue Edition",
        "type_line": "Instant",
        "mana_cost": "{R}",
        "rarity": "uncommon",
        "oracle_text": "Lightning Bolt deals 3 damage to any target.",
        "purchase_uris": {
            "cardmarket": "https://www.cardmarket.com/en/Magic/Products/Singles/Ravnica-Clue-Edition/Lightning-Bolt"
        }
    }"#;

    let card: ScryfallCard = serde_json::from_str(card_json).unwrap();
    let info = card.card_info();

    assert_eq!(info.set_name.as_deref(), Some("Ravnica: Clue Edition"));
    assert_eq!(info.type_line.as_deref(), Some("Instant"));
    assert_eq!(info.mana_cost.as_deref(), Some("{R}"));
    assert_eq!(info.rarity.as_deref(), Some("uncommon"));

    // CardInfo should serialize/deserialize for caching
    let json = serde_json::to_string(&info).unwrap();
    let deserialized: crate::scryfall::CardInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.set_name, info.set_name);
    assert_eq!(deserialized.rarity, info.rarity);
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
