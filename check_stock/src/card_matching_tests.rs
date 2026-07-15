//! Tests for card_matching.

use super::*;

fn create_test_card(name: &str, language: &str) -> Card {
    Card {
        quantity: "4".to_string(),
        name: name.to_string(),
        language: language.to_string(),
        location: Some("A-0-1-1".to_string()),
        ..Card::test_default()
    }
}

fn create_card_with_translations(
    name: &str,
    name_de: &str,
    name_es: &str,
    name_fr: &str,
    name_it: &str,
) -> Card {
    Card {
        quantity: "4".to_string(),
        name: name.to_string(),
        name_de: name_de.to_string(),
        name_es: name_es.to_string(),
        name_fr: name_fr.to_string(),
        name_it: name_it.to_string(),
        location: Some("A-0-1-1".to_string()),
        ..Card::test_default()
    }
}

// ==================== get_card_name Tests ====================

#[test]
fn test_get_card_name_no_language_returns_english() {
    let card =
        create_card_with_translations("Lightning Bolt", "Blitzschlag", "Rayo", "Éclair", "Fulmine");
    assert_eq!(get_card_name(&card, None), "Lightning Bolt");
}

#[test]
fn test_get_card_name_english_returns_english() {
    let card =
        create_card_with_translations("Lightning Bolt", "Blitzschlag", "Rayo", "Éclair", "Fulmine");
    assert_eq!(
        get_card_name(&card, Some(Language::English)),
        "Lightning Bolt"
    );
}

#[test]
fn test_get_card_name_german_returns_german() {
    let card =
        create_card_with_translations("Lightning Bolt", "Blitzschlag", "Rayo", "Éclair", "Fulmine");
    assert_eq!(get_card_name(&card, Some(Language::German)), "Blitzschlag");
}

#[test]
fn test_get_card_name_spanish_returns_spanish() {
    let card =
        create_card_with_translations("Lightning Bolt", "Blitzschlag", "Rayo", "Éclair", "Fulmine");
    assert_eq!(get_card_name(&card, Some(Language::Spanish)), "Rayo");
}

#[test]
fn test_get_card_name_french_returns_french() {
    let card =
        create_card_with_translations("Lightning Bolt", "Blitzschlag", "Rayo", "Éclair", "Fulmine");
    assert_eq!(get_card_name(&card, Some(Language::French)), "Éclair");
}

#[test]
fn test_get_card_name_italian_returns_italian() {
    let card =
        create_card_with_translations("Lightning Bolt", "Blitzschlag", "Rayo", "Éclair", "Fulmine");
    assert_eq!(get_card_name(&card, Some(Language::Italian)), "Fulmine");
}

#[test]
fn test_get_card_name_german_empty_falls_back_to_english() {
    let card = create_card_with_translations("Lightning Bolt", "", "Rayo", "Éclair", "Fulmine");
    assert_eq!(
        get_card_name(&card, Some(Language::German)),
        "Lightning Bolt"
    );
}

#[test]
fn test_get_card_name_all_translations_empty() {
    let card = create_card_with_translations("Lightning Bolt", "", "", "", "");
    assert_eq!(
        get_card_name(&card, Some(Language::German)),
        "Lightning Bolt"
    );
    assert_eq!(
        get_card_name(&card, Some(Language::Spanish)),
        "Lightning Bolt"
    );
    assert_eq!(
        get_card_name(&card, Some(Language::French)),
        "Lightning Bolt"
    );
    assert_eq!(
        get_card_name(&card, Some(Language::Italian)),
        "Lightning Bolt"
    );
}

// ==================== parse_location_code Tests ====================

#[test]
fn test_parse_location_code_simple() {
    let result = parse_location_code("A-0-1-4");
    assert_eq!(result, vec![1, 0, 1, 4]);
}

#[test]
fn test_parse_location_code_with_l0_suffix() {
    let result = parse_location_code("A-0-1-4-L0-R");
    assert_eq!(result, vec![1, 0, 1, 4]);
}

#[test]
fn test_parse_location_code_section_b() {
    let result = parse_location_code("B-1-2-3");
    assert_eq!(result, vec![2, 1, 2, 3]);
}

#[test]
fn test_parse_location_code_section_c() {
    let result = parse_location_code("C-5-10-15");
    assert_eq!(result, vec![3, 5, 10, 15]);
}

#[test]
fn test_parse_location_code_section_d() {
    let result = parse_location_code("D-0-0-0");
    assert_eq!(result, vec![4, 0, 0, 0]);
}

#[test]
fn test_parse_location_code_unknown_section() {
    let result = parse_location_code("X-1-2-3");
    assert_eq!(result, vec![0, 1, 2, 3]);
}

#[test]
fn test_parse_location_code_empty() {
    let result = parse_location_code("");
    // Empty string parses first char as 'A' default -> 1
    assert_eq!(result, vec![1]);
}

#[test]
fn test_parse_location_code_invalid_numbers() {
    let result = parse_location_code("A-x-y-z");
    assert_eq!(result, vec![1, 0, 0, 0]);
}

// ==================== find_matching_cards Tests ====================

#[test]
fn test_find_matching_cards_exact_match() {
    let inventory = vec![create_test_card("Lightning Bolt", "English")];
    let matches = find_matching_cards("Lightning Bolt", 1, &inventory, None, false);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].quantity, 1);
}

#[test]
fn test_find_matching_cards_case_insensitive() {
    let inventory = vec![create_test_card("Lightning Bolt", "English")];
    let matches = find_matching_cards("LIGHTNING BOLT", 1, &inventory, None, false);
    assert_eq!(matches.len(), 1);
}

#[test]
fn test_find_matching_cards_with_whitespace() {
    let inventory = vec![create_test_card("Lightning Bolt", "English")];
    let matches = find_matching_cards("  Lightning Bolt  ", 1, &inventory, None, false);
    assert_eq!(matches.len(), 1);
}

#[test]
fn test_find_matching_cards_no_match() {
    let inventory = vec![create_test_card("Lightning Bolt", "English")];
    let matches = find_matching_cards("Black Lotus", 1, &inventory, None, false);
    assert!(matches.is_empty());
}

#[test]
fn test_find_matching_cards_preferred_language_only() {
    let card_en = create_test_card("Lightning Bolt", "English");
    let mut card_de = create_test_card("Lightning Bolt", "German");
    card_de.name_de = "Blitzschlag".to_string();

    let inventory = vec![card_en, card_de];
    let matches = find_matching_cards(
        "Lightning Bolt",
        1,
        &inventory,
        Some(Language::English),
        true,
    );

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].card.language, "English");
}

#[test]
fn test_find_matching_cards_quantity_limiting() {
    let mut card = create_test_card("Lightning Bolt", "English");
    card.quantity = "10".to_string();

    let inventory = vec![card];
    let matches = find_matching_cards("Lightning Bolt", 3, &inventory, None, false);

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].quantity, 3); // Only takes what's needed
}

#[test]
fn test_find_matching_cards_not_enough_quantity() {
    let mut card = create_test_card("Lightning Bolt", "English");
    card.quantity = "2".to_string();

    let inventory = vec![card];
    let matches = find_matching_cards("Lightning Bolt", 5, &inventory, None, false);

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].quantity, 2); // Takes all available
}
