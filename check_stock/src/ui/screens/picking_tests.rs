//! Unit tests for the interactive picking screen

use super::*;
use crate::models::Card;

/// Helper to create a test Card with minimal required fields
fn create_test_card(
    name: &str,
    set_code: &str,
    cn: &str,
    price: &str,
    location: Option<&str>,
    foil: bool,
) -> Card {
    Card {
        cardmarket_id: "12345".to_string(),
        quantity: "1".to_string(),
        name: name.to_string(),
        set: "Test Set".to_string(),
        set_code: set_code.to_string(),
        cn: cn.to_string(),
        condition: "NM".to_string(),
        language: "English".to_string(),
        is_foil: if foil {
            "1".to_string()
        } else {
            "0".to_string()
        },
        is_playset: None,
        is_signed: "0".to_string(),
        price: price.to_string(),
        comment: "".to_string(),
        location: location.map(|s| s.to_string()),
        name_de: "".to_string(),
        name_es: "".to_string(),
        name_fr: "".to_string(),
        name_it: "".to_string(),
        rarity: "common".to_string(),
        listed_at: "2025-01-01".to_string(),
    }
}

/// Helper to create a MatchedCard for testing
fn create_matched_card<'a>(card: &'a Card, set_name: &str, quantity: i32) -> MatchedCard<'a> {
    MatchedCard {
        card,
        set_name: set_name.to_string(),
        quantity,
    }
}

// ============================================================================
// PickingItem Tests
// ============================================================================

mod picking_item_tests {
    use super::*;

    #[test]
    fn test_from_matched_card_basic() {
        let card = create_test_card("Lightning Bolt", "lea", "161", "50.00", None, false);
        let mc = create_matched_card(&card, "Limited Edition Alpha", 2);

        let item = PickingItem::from_matched_card(&mc);

        assert_eq!(item.card_name, "Lightning Bolt");
        assert_eq!(item.set_name, "Limited Edition Alpha");
        assert_eq!(item.set_code, "lea");
        assert_eq!(item.collector_number, "161");
        assert_eq!(item.condition, "NM");
        assert_eq!(item.language, "English");
        assert_eq!(item.quantity, 2);
        assert!((item.price - 50.0).abs() < 0.001);
        assert_eq!(item.location, "");
        assert!(!item.is_foil);
        assert!(!item.picked);
    }

    #[test]
    fn test_from_matched_card_with_location() {
        let card = create_test_card(
            "Black Lotus",
            "2ed",
            "232",
            "1000.00",
            Some("A1_S2_R3_C4"),
            false,
        );
        let mc = create_matched_card(&card, "Unlimited Edition", 1);

        let item = PickingItem::from_matched_card(&mc);

        assert_eq!(item.location, "A1_S2_R3_C4");
    }

    #[test]
    fn test_from_matched_card_foil() {
        let card = create_test_card("Llanowar Elves", "7ed", "253", "5.00", None, true);
        let mc = create_matched_card(&card, "Seventh Edition", 4);

        let item = PickingItem::from_matched_card(&mc);

        assert!(item.is_foil);
    }

    #[test]
    fn test_from_matched_card_invalid_price() {
        let card = Card {
            cardmarket_id: "12345".to_string(),
            quantity: "1".to_string(),
            name: "Test Card".to_string(),
            set: "Test Set".to_string(),
            set_code: "tst".to_string(),
            cn: "1".to_string(),
            condition: "NM".to_string(),
            language: "English".to_string(),
            is_foil: "0".to_string(),
            is_playset: None,
            is_signed: "0".to_string(),
            price: "invalid".to_string(),
            comment: "".to_string(),
            location: None,
            name_de: "".to_string(),
            name_es: "".to_string(),
            name_fr: "".to_string(),
            name_it: "".to_string(),
            rarity: "common".to_string(),
            listed_at: "2025-01-01".to_string(),
        };
        let mc = create_matched_card(&card, "Test Set", 1);

        let item = PickingItem::from_matched_card(&mc);

        assert!((item.price - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_image_key_format() {
        let card = create_test_card("Sol Ring", "CMD", "123", "1.00", None, false);
        let mc = create_matched_card(&card, "Commander", 1);

        let item = PickingItem::from_matched_card(&mc);

        assert_eq!(item.image_key(), "cmd_123");
    }

    #[test]
    fn test_image_key_lowercase() {
        let card = create_test_card("Test", "MH2", "ABC", "1.00", None, false);
        let mc = create_matched_card(&card, "Modern Horizons 2", 1);

        let item = PickingItem::from_matched_card(&mc);

        assert_eq!(item.image_key(), "mh2_ABC");
    }

    #[test]
    fn test_image_key_special_collector_number() {
        let card = create_test_card("Test", "uno", "1★", "1.00", None, false);
        let mc = create_matched_card(&card, "Unfinity", 1);

        let item = PickingItem::from_matched_card(&mc);

        assert_eq!(item.image_key(), "uno_1★");
    }

    #[test]
    fn test_picking_item_clone() {
        let card = create_test_card("Clone", "m14", "47", "0.50", Some("B1_S1_R1_C1"), false);
        let mc = create_matched_card(&card, "Magic 2014", 3);

        let item = PickingItem::from_matched_card(&mc);
        let cloned = item.clone();

        assert_eq!(item.card_name, cloned.card_name);
        assert_eq!(item.set_code, cloned.set_code);
        assert_eq!(item.quantity, cloned.quantity);
        assert_eq!(item.picked, cloned.picked);
    }
}

// ============================================================================
// PickingState Tests
// ============================================================================

mod picking_state_tests {
    use super::*;

    #[test]
    fn test_default_state() {
        let state = PickingState::default();

        assert!(state.items.is_empty());
        assert!(state.images.is_empty());
        assert!(state.loading_images.is_empty());
        assert!(!state.show_picked);
        assert!((state.total_price - 0.0).abs() < 0.001);
        assert!((state.picked_price - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_from_matched_cards_empty() {
        let matches: Vec<(String, i32, Vec<MatchedCard>)> = vec![];
        let state = PickingState::from_matched_cards(&matches);

        assert!(state.items.is_empty());
        assert!((state.total_price - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_from_matched_cards_single_card() {
        let card = create_test_card("Counterspell", "ice", "64", "2.50", None, false);
        let mc = create_matched_card(&card, "Ice Age", 1);
        let matches = vec![("Counterspell".to_string(), 1, vec![mc])];

        let state = PickingState::from_matched_cards(&matches);

        assert_eq!(state.items.len(), 1);
        assert_eq!(state.items[0].card_name, "Counterspell");
        assert!((state.total_price - 2.50).abs() < 0.001);
    }

    #[test]
    fn test_from_matched_cards_multiple_cards() {
        let card1 = create_test_card("Brainstorm", "ice", "61", "1.00", None, false);
        let card2 = create_test_card("Dark Ritual", "ice", "117", "2.00", None, false);
        let mc1 = create_matched_card(&card1, "Ice Age", 4);
        let mc2 = create_matched_card(&card2, "Ice Age", 4);
        let matches = vec![
            ("Brainstorm".to_string(), 4, vec![mc1]),
            ("Dark Ritual".to_string(), 4, vec![mc2]),
        ];

        let state = PickingState::from_matched_cards(&matches);

        assert_eq!(state.items.len(), 2);
        // 4 * 1.00 + 4 * 2.00 = 12.00
        assert!((state.total_price - 12.0).abs() < 0.001);
    }

    #[test]
    fn test_from_matched_cards_sorted_by_location() {
        let card1 = create_test_card("Card A", "tst", "1", "1.00", Some("C1_S1_R1_C1"), false);
        let card2 = create_test_card("Card B", "tst", "2", "1.00", Some("A1_S1_R1_C1"), false);
        let card3 = create_test_card("Card C", "tst", "3", "1.00", Some("B1_S1_R1_C1"), false);
        let mc1 = create_matched_card(&card1, "Test", 1);
        let mc2 = create_matched_card(&card2, "Test", 1);
        let mc3 = create_matched_card(&card3, "Test", 1);
        let matches = vec![
            ("Card A".to_string(), 1, vec![mc1]),
            ("Card B".to_string(), 1, vec![mc2]),
            ("Card C".to_string(), 1, vec![mc3]),
        ];

        let state = PickingState::from_matched_cards(&matches);

        // Should be sorted: A1 < B1 < C1
        assert_eq!(state.items[0].card_name, "Card B"); // A1
        assert_eq!(state.items[1].card_name, "Card C"); // B1
        assert_eq!(state.items[2].card_name, "Card A"); // C1
    }

    #[test]
    fn test_from_matched_cards_empty_locations_first() {
        let card1 = create_test_card("Card A", "tst", "1", "1.00", Some("A1_S1_R1_C1"), false);
        let card2 = create_test_card("Card B", "tst", "2", "1.00", None, false);
        let mc1 = create_matched_card(&card1, "Test", 1);
        let mc2 = create_matched_card(&card2, "Test", 1);
        let matches = vec![
            ("Card A".to_string(), 1, vec![mc1]),
            ("Card B".to_string(), 1, vec![mc2]),
        ];

        let state = PickingState::from_matched_cards(&matches);

        // Empty string sorts before "A1..."
        assert_eq!(state.items[0].card_name, "Card B"); // empty location
        assert_eq!(state.items[1].card_name, "Card A"); // A1
    }

    #[test]
    fn test_picked_count_none_picked() {
        let card = create_test_card("Test", "tst", "1", "1.00", None, false);
        let mc = create_matched_card(&card, "Test", 1);
        let matches = vec![("Test".to_string(), 1, vec![mc])];

        let state = PickingState::from_matched_cards(&matches);

        assert_eq!(state.picked_count(), 0);
    }

    #[test]
    fn test_picked_count_some_picked() {
        let card1 = create_test_card("Card A", "tst", "1", "1.00", None, false);
        let card2 = create_test_card("Card B", "tst", "2", "1.00", None, false);
        let card3 = create_test_card("Card C", "tst", "3", "1.00", None, false);
        let mc1 = create_matched_card(&card1, "Test", 1);
        let mc2 = create_matched_card(&card2, "Test", 1);
        let mc3 = create_matched_card(&card3, "Test", 1);
        let matches = vec![
            ("Card A".to_string(), 1, vec![mc1]),
            ("Card B".to_string(), 1, vec![mc2]),
            ("Card C".to_string(), 1, vec![mc3]),
        ];

        let mut state = PickingState::from_matched_cards(&matches);
        state.items[0].picked = true;
        state.items[2].picked = true;

        assert_eq!(state.picked_count(), 2);
    }

    #[test]
    fn test_picked_count_all_picked() {
        let card = create_test_card("Test", "tst", "1", "1.00", None, false);
        let mc = create_matched_card(&card, "Test", 1);
        let matches = vec![("Test".to_string(), 1, vec![mc])];

        let mut state = PickingState::from_matched_cards(&matches);
        state.items[0].picked = true;

        assert_eq!(state.picked_count(), 1);
        assert_eq!(state.picked_count(), state.total_count());
    }

    #[test]
    fn test_total_count() {
        let card1 = create_test_card("Card A", "tst", "1", "1.00", None, false);
        let card2 = create_test_card("Card B", "tst", "2", "1.00", None, false);
        let mc1 = create_matched_card(&card1, "Test", 1);
        let mc2 = create_matched_card(&card2, "Test", 1);
        let matches = vec![
            ("Card A".to_string(), 1, vec![mc1]),
            ("Card B".to_string(), 1, vec![mc2]),
        ];

        let state = PickingState::from_matched_cards(&matches);

        assert_eq!(state.total_count(), 2);
    }

    #[test]
    fn test_total_count_empty() {
        let state = PickingState::default();

        assert_eq!(state.total_count(), 0);
    }

    #[test]
    fn test_update_picked_price_none_picked() {
        let card = create_test_card("Test", "tst", "1", "10.00", None, false);
        let mc = create_matched_card(&card, "Test", 1);
        let matches = vec![("Test".to_string(), 1, vec![mc])];

        let mut state = PickingState::from_matched_cards(&matches);
        state.update_picked_price();

        assert!((state.picked_price - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_update_picked_price_some_picked() {
        let card1 = create_test_card("Card A", "tst", "1", "5.00", None, false);
        let card2 = create_test_card("Card B", "tst", "2", "3.00", None, false);
        let mc1 = create_matched_card(&card1, "Test", 2); // 2 * 5.00 = 10.00
        let mc2 = create_matched_card(&card2, "Test", 3); // 3 * 3.00 = 9.00
        let matches = vec![
            ("Card A".to_string(), 2, vec![mc1]),
            ("Card B".to_string(), 3, vec![mc2]),
        ];

        let mut state = PickingState::from_matched_cards(&matches);
        state.items[0].picked = true; // Pick Card A: 10.00
        state.update_picked_price();

        assert!((state.picked_price - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_update_picked_price_all_picked() {
        let card = create_test_card("Test", "tst", "1", "7.50", None, false);
        let mc = create_matched_card(&card, "Test", 4);
        let matches = vec![("Test".to_string(), 4, vec![mc])];

        let mut state = PickingState::from_matched_cards(&matches);
        state.items[0].picked = true;
        state.update_picked_price();

        // 4 * 7.50 = 30.00
        assert!((state.picked_price - 30.0).abs() < 0.001);
        assert!((state.picked_price - state.total_price).abs() < 0.001);
    }

    #[test]
    fn test_update_picked_price_updates_correctly_after_unpick() {
        let card1 = create_test_card("Card A", "tst", "1", "10.00", None, false);
        let card2 = create_test_card("Card B", "tst", "2", "20.00", None, false);
        let mc1 = create_matched_card(&card1, "Test", 1);
        let mc2 = create_matched_card(&card2, "Test", 1);
        let matches = vec![
            ("Card A".to_string(), 1, vec![mc1]),
            ("Card B".to_string(), 1, vec![mc2]),
        ];

        let mut state = PickingState::from_matched_cards(&matches);

        // Pick both
        state.items[0].picked = true;
        state.items[1].picked = true;
        state.update_picked_price();
        assert!((state.picked_price - 30.0).abs() < 0.001);

        // Unpick Card A
        state.items[0].picked = false;
        state.update_picked_price();
        assert!((state.picked_price - 20.0).abs() < 0.001);
    }

    #[test]
    fn test_total_price_with_quantities() {
        let card1 = create_test_card("Expensive", "tst", "1", "100.00", None, false);
        let card2 = create_test_card("Cheap", "tst", "2", "0.50", None, false);
        let mc1 = create_matched_card(&card1, "Test", 1); // 1 * 100.00 = 100.00
        let mc2 = create_matched_card(&card2, "Test", 8); // 8 * 0.50 = 4.00
        let matches = vec![
            ("Expensive".to_string(), 1, vec![mc1]),
            ("Cheap".to_string(), 8, vec![mc2]),
        ];

        let state = PickingState::from_matched_cards(&matches);

        assert!((state.total_price - 104.0).abs() < 0.001);
    }
}

// ============================================================================
// LoadedImage Tests
// ============================================================================

mod loaded_image_tests {
    use super::*;

    #[test]
    fn test_loaded_image_creation() {
        let loaded = LoadedImage {
            image_key: "cmd_123".to_string(),
            set_code: "cmd".to_string(),
            collector_number: "123".to_string(),
            image_data: vec![0xFF, 0xD8, 0xFF], // JPEG magic bytes
        };

        assert_eq!(loaded.image_key, "cmd_123");
        assert_eq!(loaded.set_code, "cmd");
        assert_eq!(loaded.collector_number, "123");
        assert_eq!(loaded.image_data.len(), 3);
    }

    #[test]
    fn test_loaded_image_empty_data() {
        let loaded = LoadedImage {
            image_key: "test_1".to_string(),
            set_code: "test".to_string(),
            collector_number: "1".to_string(),
            image_data: vec![],
        };

        assert!(loaded.image_data.is_empty());
    }
}

// ============================================================================
// Integration-style Tests
// ============================================================================

mod integration_tests {
    use super::*;

    #[test]
    fn test_full_picking_workflow() {
        // Simulate a full picking workflow
        let card1 = create_test_card("Sol Ring", "cmd", "237", "2.00", Some("A1_S1_R1_C1"), false);
        let card2 = create_test_card(
            "Command Tower",
            "cmd",
            "281",
            "1.50",
            Some("A1_S1_R1_C2"),
            false,
        );
        let card3 = create_test_card(
            "Arcane Signet",
            "eld",
            "331",
            "1.00",
            Some("A1_S1_R2_C1"),
            false,
        );

        let mc1 = create_matched_card(&card1, "Commander", 1);
        let mc2 = create_matched_card(&card2, "Commander", 1);
        let mc3 = create_matched_card(&card3, "Throne of Eldraine", 1);

        let matches = vec![
            ("Sol Ring".to_string(), 1, vec![mc1]),
            ("Command Tower".to_string(), 1, vec![mc2]),
            ("Arcane Signet".to_string(), 1, vec![mc3]),
        ];

        let mut state = PickingState::from_matched_cards(&matches);

        // Initial state
        assert_eq!(state.total_count(), 3);
        assert_eq!(state.picked_count(), 0);
        assert!((state.total_price - 4.50).abs() < 0.001);
        assert!((state.picked_price - 0.0).abs() < 0.001);

        // Pick first item
        state.items[0].picked = true;
        state.update_picked_price();
        assert_eq!(state.picked_count(), 1);

        // Pick second item
        state.items[1].picked = true;
        state.update_picked_price();
        assert_eq!(state.picked_count(), 2);

        // Pick all
        state.items[2].picked = true;
        state.update_picked_price();
        assert_eq!(state.picked_count(), 3);
        assert!((state.picked_price - state.total_price).abs() < 0.001);

        // Unpick one
        state.items[1].picked = false;
        state.update_picked_price();
        assert_eq!(state.picked_count(), 2);
    }

    #[test]
    fn test_multiple_cards_same_match_group() {
        // Test when a wanted card is fulfilled by multiple cards
        let card1 = create_test_card("Lightning Bolt", "lea", "161", "100.00", None, false);
        let card2 = create_test_card("Lightning Bolt", "m10", "146", "1.00", None, false);
        let mc1 = create_matched_card(&card1, "Alpha", 1);
        let mc2 = create_matched_card(&card2, "Magic 2010", 3);

        // Both cards in the same match group (user wanted 4 Lightning Bolts)
        let matches = vec![("Lightning Bolt".to_string(), 4, vec![mc1, mc2])];

        let state = PickingState::from_matched_cards(&matches);

        assert_eq!(state.total_count(), 2); // Two different printings
                                            // Total: 1 * 100.00 + 3 * 1.00 = 103.00
        assert!((state.total_price - 103.0).abs() < 0.001);
    }

    #[test]
    fn test_foil_and_nonfoil_same_card() {
        let card1 = create_test_card("Path to Exile", "con", "15", "5.00", None, false);
        let card2 = create_test_card("Path to Exile", "con", "15", "15.00", None, true);
        let mc1 = create_matched_card(&card1, "Conflux", 1);
        let mc2 = create_matched_card(&card2, "Conflux", 1);

        let matches = vec![("Path to Exile".to_string(), 2, vec![mc1, mc2])];

        let state = PickingState::from_matched_cards(&matches);

        assert_eq!(state.total_count(), 2);
        assert!(!state.items[0].is_foil || !state.items[1].is_foil); // One is non-foil
        assert!(state.items[0].is_foil || state.items[1].is_foil); // One is foil
    }
}
