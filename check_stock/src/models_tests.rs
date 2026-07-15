//! Tests for models.

use super::*;

fn create_test_card() -> Card {
    Card {
        quantity: "4".to_string(),
        name: "Lightning Bolt".to_string(),
        set: "Alpha".to_string(),
        set_code: "LEA".to_string(),
        cn: "123".to_string(),
        price: "25.50".to_string(),
        location: Some("A-0-1-1".to_string()),
        name_de: "Blitzschlag".to_string(),
        name_es: "Rayo".to_string(),
        name_fr: "Éclair".to_string(),
        name_it: "Fulmine".to_string(),
        ..Card::test_default()
    }
}

// ==================== Language Tests ====================

#[test]
fn test_language_as_str() {
    assert_eq!(Language::English.as_str(), "English");
    assert_eq!(Language::German.as_str(), "German");
    assert_eq!(Language::Spanish.as_str(), "Spanish");
    assert_eq!(Language::French.as_str(), "French");
    assert_eq!(Language::Italian.as_str(), "Italian");
}

#[test]
fn test_language_code() {
    assert_eq!(Language::English.code(), "en");
    assert_eq!(Language::German.code(), "de");
    assert_eq!(Language::Spanish.code(), "es");
    assert_eq!(Language::French.code(), "fr");
    assert_eq!(Language::Italian.code(), "it");
}

#[test]
fn test_language_from_code_valid() {
    assert_eq!(Language::from_code("en"), Some(Language::English));
    assert_eq!(Language::from_code("de"), Some(Language::German));
    assert_eq!(Language::from_code("es"), Some(Language::Spanish));
    assert_eq!(Language::from_code("fr"), Some(Language::French));
    assert_eq!(Language::from_code("it"), Some(Language::Italian));
}

#[test]
fn test_language_from_code_case_insensitive() {
    assert_eq!(Language::from_code("EN"), Some(Language::English));
    assert_eq!(Language::from_code("De"), Some(Language::German));
    assert_eq!(Language::from_code("ES"), Some(Language::Spanish));
}

#[test]
fn test_language_from_code_invalid() {
    assert_eq!(Language::from_code("xx"), None);
    assert_eq!(Language::from_code(""), None);
    assert_eq!(Language::from_code("english"), None); // full name, not code
}

#[test]
fn test_language_from_full_name_valid() {
    assert_eq!(Language::from_full_name("English"), Some(Language::English));
    assert_eq!(Language::from_full_name("German"), Some(Language::German));
    assert_eq!(Language::from_full_name("Spanish"), Some(Language::Spanish));
    assert_eq!(Language::from_full_name("French"), Some(Language::French));
    assert_eq!(Language::from_full_name("Italian"), Some(Language::Italian));
}

#[test]
fn test_language_from_full_name_case_insensitive() {
    assert_eq!(Language::from_full_name("ENGLISH"), Some(Language::English));
    assert_eq!(Language::from_full_name("german"), Some(Language::German));
    assert_eq!(Language::from_full_name("SpAnIsH"), Some(Language::Spanish));
}

#[test]
fn test_language_from_full_name_invalid() {
    assert_eq!(Language::from_full_name("en"), None); // code, not full name
    assert_eq!(Language::from_full_name(""), None);
    assert_eq!(Language::from_full_name("Japanese"), None);
}

#[test]
fn test_language_parse_accepts_both_code_and_name() {
    // Codes
    assert_eq!(Language::parse("en"), Some(Language::English));
    assert_eq!(Language::parse("de"), Some(Language::German));
    // Full names
    assert_eq!(Language::parse("English"), Some(Language::English));
    assert_eq!(Language::parse("German"), Some(Language::German));
    // Invalid
    assert_eq!(Language::parse("xx"), None);
    assert_eq!(Language::parse(""), None);
}

#[test]
fn test_language_all() {
    let all = Language::all();
    assert_eq!(all.len(), 5);
    assert!(all.contains(&Language::English));
    assert!(all.contains(&Language::German));
    assert!(all.contains(&Language::Spanish));
    assert!(all.contains(&Language::French));
    assert!(all.contains(&Language::Italian));
}

// ==================== Card Tests ====================

#[test]
fn test_card_is_foil_false() {
    let card = create_test_card();
    assert!(!card.is_foil_card());
}

#[test]
fn test_card_is_foil_true_with_1() {
    let mut card = create_test_card();
    card.is_foil = "1".to_string();
    assert!(card.is_foil_card());
}

#[test]
fn test_card_is_foil_true_with_true() {
    let mut card = create_test_card();
    card.is_foil = "true".to_string();
    assert!(card.is_foil_card());
}

#[test]
fn test_card_is_foil_true_case_insensitive() {
    let mut card = create_test_card();
    card.is_foil = "TRUE".to_string();
    assert!(card.is_foil_card());
}

#[test]
fn test_card_is_signed_false() {
    let card = create_test_card();
    assert!(!card.is_signed_card());
}

#[test]
fn test_card_is_signed_true() {
    let mut card = create_test_card();
    card.is_signed = "1".to_string();
    assert!(card.is_signed_card());
}

#[test]
fn test_card_is_playset_none() {
    let card = create_test_card();
    assert!(!card.is_playset_card());
}

#[test]
fn test_card_is_playset_false() {
    let mut card = create_test_card();
    card.is_playset = Some("false".to_string());
    assert!(!card.is_playset_card());
}

#[test]
fn test_card_is_playset_true() {
    let mut card = create_test_card();
    card.is_playset = Some("1".to_string());
    assert!(card.is_playset_card());
}

#[test]
fn test_card_special_conditions_none() {
    let card = create_test_card();
    assert!(card.special_conditions().is_empty());
}

#[test]
fn test_card_special_conditions_foil_only() {
    let mut card = create_test_card();
    card.is_foil = "true".to_string();
    let conditions = card.special_conditions();
    assert_eq!(conditions, vec!["Foil"]);
}

#[test]
fn test_card_special_conditions_signed_only() {
    let mut card = create_test_card();
    card.is_signed = "true".to_string();
    let conditions = card.special_conditions();
    assert_eq!(conditions, vec!["Signed"]);
}

#[test]
fn test_card_special_conditions_both() {
    let mut card = create_test_card();
    card.is_foil = "true".to_string();
    card.is_signed = "true".to_string();
    let conditions = card.special_conditions();
    assert_eq!(conditions, vec!["Foil", "Signed"]);
}

#[test]
fn test_card_is_first_ed_none() {
    let card = create_test_card();
    assert!(!card.is_first_ed_card());
}

#[test]
fn test_card_is_first_ed_true() {
    let mut card = create_test_card();
    card.is_first_ed = Some("true".to_string());
    assert!(card.is_first_ed_card());
}

#[test]
fn test_card_is_reverse_holo_true() {
    let mut card = create_test_card();
    card.is_reverse_holo = Some("true".to_string());
    assert!(card.is_reverse_holo_card());
}

#[test]
fn test_card_special_conditions_first_ed_and_reverse_holo() {
    let mut card = create_test_card();
    card.is_first_ed = Some("true".to_string());
    card.is_reverse_holo = Some("true".to_string());
    let conditions = card.special_conditions();
    assert_eq!(conditions, vec!["1st Ed", "Reverse Holo"]);
}

// ==================== canonical_condition Tests ====================

#[test]
fn test_canonical_condition_short_form() {
    assert_eq!(canonical_condition("NM"), "NM");
    assert_eq!(canonical_condition("EX"), "EX");
    assert_eq!(canonical_condition("LP"), "LP");
}

#[test]
fn test_canonical_condition_long_form() {
    assert_eq!(canonical_condition("near_mint"), "NM");
    assert_eq!(canonical_condition("excellent"), "EX");
    assert_eq!(canonical_condition("good"), "GD");
    assert_eq!(canonical_condition("light_played"), "LP");
    assert_eq!(canonical_condition("played"), "PL");
    assert_eq!(canonical_condition("poor"), "PO");
}

#[test]
fn test_canonical_condition_case_insensitive() {
    assert_eq!(canonical_condition("Near_Mint"), "NM");
    assert_eq!(canonical_condition("nm"), "NM");
    assert_eq!(canonical_condition("NEAR_MINT"), "NM");
}

#[test]
fn test_canonical_condition_whitespace_and_hyphen() {
    assert_eq!(canonical_condition("near mint"), "NM");
    assert_eq!(canonical_condition("light-played"), "LP");
    assert_eq!(canonical_condition(" NM "), "NM");
}

#[test]
fn test_canonical_condition_unknown_returned_uppercased() {
    assert_eq!(canonical_condition("weird"), "WEIRD");
}

#[test]
fn test_card_price_f64_valid() {
    let card = create_test_card();
    assert!((card.price_f64() - 25.50).abs() < 0.001);
}

#[test]
fn test_card_price_f64_integer() {
    let mut card = create_test_card();
    card.price = "100".to_string();
    assert!((card.price_f64() - 100.0).abs() < 0.001);
}

#[test]
fn test_card_price_f64_invalid() {
    let mut card = create_test_card();
    card.price = "not_a_number".to_string();
    assert_eq!(card.price_f64(), 0.0);
}

#[test]
fn test_card_price_f64_empty() {
    let mut card = create_test_card();
    card.price = "".to_string();
    assert_eq!(card.price_f64(), 0.0);
}
