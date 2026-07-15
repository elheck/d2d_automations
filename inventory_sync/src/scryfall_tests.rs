//! Tests for Scryfall API client
//!
//! Note: Some tests require network access and are marked with #[ignore]

use crate::scryfall::{CardInfo, ScryfallCard};

/// Minimal identity fields required by the shared ScryfallCard struct.
fn base_card_json(extra: &str) -> String {
    format!(
        r#"{{
        "id": "test-uuid",
        "name": "Test Card",
        "set": "tst",
        "set_name": "Test Set",
        "collector_number": "1",
        "rarity": "common"{}{}
    }}"#,
        if extra.is_empty() { "" } else { "," },
        extra
    )
}

#[test]
fn test_scryfall_card_image_url_direct() {
    let card_json = base_card_json(
        r#""image_uris": {
            "normal": "https://example.com/normal.jpg",
            "large": "https://example.com/large.jpg"
        }"#,
    );

    let card: ScryfallCard = serde_json::from_str(&card_json).unwrap();
    assert_eq!(card.image_url(), Some("https://example.com/normal.jpg"));
}

#[test]
fn test_scryfall_card_image_url_double_faced() {
    let card_json = base_card_json(
        r#""card_faces": [
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
        ]"#,
    );

    let card: ScryfallCard = serde_json::from_str(&card_json).unwrap();
    // Should return front face image
    assert_eq!(card.image_url(), Some("https://example.com/front.jpg"));
}

#[test]
fn test_scryfall_card_image_url_none() {
    let card_json = base_card_json("");

    let card: ScryfallCard = serde_json::from_str(&card_json).unwrap();
    assert_eq!(card.image_url(), None);
}

#[test]
fn test_scryfall_card_deserialize_without_optional_fields() {
    let card_json = base_card_json("");

    let card: ScryfallCard = serde_json::from_str(&card_json).unwrap();
    assert_eq!(card.name, "Test Card");
    assert!(card.image_uris.is_none());
    assert!(card.card_faces.is_none());
    assert!(card.purchase_uris.is_none());
}

#[test]
fn test_scryfall_card_with_metadata() {
    let card_json = r#"{
        "id": "test-uuid",
        "name": "Lightning Bolt",
        "set": "clu",
        "set_name": "Ravnica: Clue Edition",
        "collector_number": "141",
        "rarity": "uncommon",
        "type_line": "Instant",
        "mana_cost": "{R}",
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
    assert_eq!(card.set_name, "Ravnica: Clue Edition");
    assert_eq!(card.type_line.as_deref(), Some("Instant"));
    assert_eq!(card.mana_cost.as_deref(), Some("{R}"));
    assert_eq!(card.rarity, "uncommon");
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
        "id": "test-uuid",
        "name": "Lightning Bolt",
        "set": "clu",
        "set_name": "Ravnica: Clue Edition",
        "collector_number": "141",
        "rarity": "uncommon",
        "type_line": "Instant",
        "mana_cost": "{R}",
        "oracle_text": "Lightning Bolt deals 3 damage to any target.",
        "purchase_uris": {
            "cardmarket": "https://www.cardmarket.com/en/Magic/Products/Singles/Ravnica-Clue-Edition/Lightning-Bolt"
        }
    }"#;

    let card: ScryfallCard = serde_json::from_str(card_json).unwrap();
    let info = CardInfo::from(&card);

    assert_eq!(info.set_name.as_deref(), Some("Ravnica: Clue Edition"));
    assert_eq!(info.type_line.as_deref(), Some("Instant"));
    assert_eq!(info.mana_cost.as_deref(), Some("{R}"));
    assert_eq!(info.rarity.as_deref(), Some("uncommon"));

    // CardInfo should serialize/deserialize for caching
    let json = serde_json::to_string(&info).unwrap();
    let deserialized: CardInfo = serde_json::from_str(&json).unwrap();
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
