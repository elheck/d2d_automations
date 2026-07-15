//! Tests for formatters.

use super::*;
use crate::models::Card;

fn create_test_card(name: &str, price: &str, quantity: i32) -> Card {
    Card {
        name: name.to_string(),
        price: price.to_string(),
        quantity: quantity.to_string(),
        location: Some("A-0-1-1".to_string()),
        ..Card::test_default()
    }
}

fn create_matched_card<'a>(card: &'a Card, quantity: i32, set_name: &str) -> MatchedCard<'a> {
    MatchedCard {
        card,
        quantity,
        set_name: set_name.to_string(),
    }
}

// ==================== format_regular_output Tests ====================

#[test]
fn test_format_regular_output_empty() {
    let matches: Vec<(String, i32, Vec<MatchedCard>)> = vec![];
    let output = format_regular_output(&matches, 0.0);
    assert!(output.contains("No cards from your wantslist were found"));
}

#[test]
fn test_format_regular_output_single_card() {
    let card = create_test_card("Lightning Bolt", "10.00", 4);
    let matched = create_matched_card(&card, 1, "Alpha (LEA)");

    let matches = vec![("Lightning Bolt".to_string(), 1, vec![matched])];
    let output = format_regular_output(&matches, 0.0);

    assert!(output.contains("1 x Lightning Bolt"));
    assert!(output.contains("10.00 €"));
    assert!(output.contains("Alpha (LEA)"));
    assert!(output.contains("NM condition"));
}

#[test]
fn test_format_regular_output_with_discount() {
    let card = create_test_card("Lightning Bolt", "100.00", 4);
    let matched = create_matched_card(&card, 1, "Alpha (LEA)");

    let matches = vec![("Lightning Bolt".to_string(), 1, vec![matched])];
    let output = format_regular_output(&matches, 10.0);

    assert!(output.contains("90.00 €")); // 10% discount
    assert!(output.contains("10.0% discount"));
}

#[test]
fn test_format_regular_output_partial_availability() {
    let card = create_test_card("Lightning Bolt", "10.00", 2);
    let matched = create_matched_card(&card, 2, "Alpha (LEA)");

    let matches = vec![("Lightning Bolt".to_string(), 4, vec![matched])];
    let output = format_regular_output(&matches, 0.0);

    assert!(output.contains("2 of 4"));
    assert!(output.contains("WARNING: Only 2 of 4 copies available"));
}

#[test]
fn test_format_regular_output_foil_card() {
    let mut card = create_test_card("Lightning Bolt", "50.00", 4);
    card.is_foil = "true".to_string();
    let matched = create_matched_card(&card, 1, "Alpha (LEA)");

    let matches = vec![("Lightning Bolt".to_string(), 1, vec![matched])];
    let output = format_regular_output(&matches, 0.0);

    assert!(output.contains("(Foil)"));
}

#[test]
fn test_format_regular_output_signed_card() {
    let mut card = create_test_card("Lightning Bolt", "100.00", 4);
    card.is_signed = "true".to_string();
    let matched = create_matched_card(&card, 1, "Alpha (LEA)");

    let matches = vec![("Lightning Bolt".to_string(), 1, vec![matched])];
    let output = format_regular_output(&matches, 0.0);

    assert!(output.contains("(Signed)"));
}

#[test]
fn test_format_regular_output_with_comment() {
    let mut card = create_test_card("Lightning Bolt", "10.00", 4);
    card.comment = "Great condition!".to_string();
    let matched = create_matched_card(&card, 1, "Alpha (LEA)");

    let matches = vec![("Lightning Bolt".to_string(), 1, vec![matched])];
    let output = format_regular_output(&matches, 0.0);

    assert!(output.contains("Note: Great condition!"));
}

#[test]
fn test_format_regular_output_with_location() {
    let card = create_test_card("Lightning Bolt", "10.00", 4);
    let matched = create_matched_card(&card, 1, "Alpha (LEA)");

    let matches = vec![("Lightning Bolt".to_string(), 1, vec![matched])];
    let output = format_regular_output(&matches, 0.0);

    assert!(output.contains("[Location: A-0-1-1]"));
}

#[test]
fn test_format_regular_output_total_price() {
    let card1 = create_test_card("Lightning Bolt", "10.00", 4);
    let card2 = create_test_card("Black Lotus", "90.00", 1);
    let matched1 = create_matched_card(&card1, 2, "Alpha (LEA)");
    let matched2 = create_matched_card(&card2, 1, "Alpha (LEA)");

    let matches = vec![
        ("Lightning Bolt".to_string(), 2, vec![matched1]),
        ("Black Lotus".to_string(), 1, vec![matched2]),
    ];
    let output = format_regular_output(&matches, 0.0);

    assert!(output.contains("Total price for available cards: 110.00 €"));
    assert!(output.contains("Total cards picked: 3"));
}

// ==================== format_picking_list Tests ====================

#[test]
fn test_format_picking_list_empty() {
    let cards: Vec<MatchedCard> = vec![];
    let output = format_picking_list(&cards);

    // Should have header row
    assert!(output.contains("Qty"));
    assert!(output.contains("Location"));
}

#[test]
fn test_format_picking_list_single_card() {
    let card = create_test_card("Lightning Bolt", "10.00", 4);
    let matched = create_matched_card(&card, 2, "Alpha (LEA)");

    let output = format_picking_list(&[matched]);

    assert!(output.contains("Lightning Bolt"));
    assert!(output.contains("A-0-1-1"));
    assert!(output.contains("2")); // quantity
}

#[test]
fn test_format_picking_list_sorted_by_location() {
    let mut card1 = create_test_card("Card A", "10.00", 4);
    card1.location = Some("B-0-1-1".to_string());
    let mut card2 = create_test_card("Card B", "10.00", 4);
    card2.location = Some("A-0-1-1".to_string());

    let matched1 = create_matched_card(&card1, 1, "Set");
    let matched2 = create_matched_card(&card2, 1, "Set");

    let output = format_picking_list(&[matched1, matched2]);

    // A location should come before B location
    let a_pos = output.find("A-0-1-1").unwrap();
    let b_pos = output.find("B-0-1-1").unwrap();
    assert!(a_pos < b_pos);
}

#[test]
fn test_format_picking_list_german_card_uses_german_name() {
    let mut card = create_test_card("Lightning Bolt", "10.00", 4);
    card.language = "German".to_string();
    card.name_de = "Blitzschlag".to_string();

    let matched = create_matched_card(&card, 1, "Alpha (LEA)");
    let output = format_picking_list(&[matched]);

    assert!(output.contains("Blitzschlag"));
}

// ==================== format_invoice_list Tests ====================

#[test]
fn test_format_invoice_list_empty() {
    let cards: Vec<MatchedCard> = vec![];
    let output = format_invoice_list(&cards);

    assert!(output.contains("Qty"));
    assert!(output.contains("Name"));
}

#[test]
fn test_format_invoice_list_single_card() {
    let card = create_test_card("Lightning Bolt", "100.00", 4);
    let matched = create_matched_card(&card, 1, "Alpha (LEA)");

    let output = format_invoice_list(&[matched]);

    assert!(output.contains("Lightning Bolt"));
    assert!(output.contains("100.00"));
}

#[test]
fn test_format_invoice_list_total() {
    let card = create_test_card("Lightning Bolt", "25.00", 4);
    let matched = create_matched_card(&card, 2, "Alpha (LEA)");

    let output = format_invoice_list(&[matched]);

    // 2 cards at 25.00 = 50.00 total
    assert!(output.contains("50.00"));
}

// ==================== format_price_diff_csv Tests ====================

#[test]
fn test_price_diff_csv_empty_overrides() {
    let cards = vec![create_test_card("Card A", "1.00", 1)];
    let overrides = std::collections::HashMap::new();
    let output = format_price_diff_csv(&cards, &[0], &overrides);

    // Header only, no data rows
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("cardmarketId"));
}

#[test]
fn test_price_diff_csv_only_changed_cards() {
    let cards = vec![
        create_test_card("Unchanged", "5.00", 1),
        create_test_card("Changed", "3.00", 1),
        create_test_card("Also Unchanged", "7.00", 1),
    ];
    let mut overrides = std::collections::HashMap::new();
    overrides.insert(1, 4.50);

    let output = format_price_diff_csv(&cards, &[0, 1, 2], &overrides);

    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2); // header + 1 changed card
    assert!(lines[1].contains("Changed"));
    assert!(lines[1].contains("4.50"));
    assert!(!output.contains("Unchanged"));
}

#[test]
fn test_price_diff_csv_preserves_card_fields() {
    let mut card = create_test_card("Lightning Bolt", "10.00", 2);
    card.cardmarket_id = "99999".to_string();
    card.set = "Alpha".to_string();
    card.condition = "EX".to_string();
    card.language = "German".to_string();
    card.is_foil = "true".to_string();
    card.location = Some("A1_S2_R3_C4".to_string());
    let cards = vec![card];

    let mut overrides = std::collections::HashMap::new();
    overrides.insert(0, 15.00);

    let output = format_price_diff_csv(&cards, &[0], &overrides);

    assert!(output.contains("99999"));
    assert!(output.contains("Lightning Bolt"));
    assert!(output.contains("Alpha"));
    assert!(output.contains("EX"));
    assert!(output.contains("German"));
    assert!(output.contains("true"));
    assert!(output.contains("A1_S2_R3_C4"));
    assert!(output.contains("15.00"));
}

#[test]
fn test_price_diff_csv_skips_indices_not_in_overrides() {
    let cards = vec![
        create_test_card("Card A", "1.00", 1),
        create_test_card("Card B", "2.00", 1),
    ];
    let mut overrides = std::collections::HashMap::new();
    overrides.insert(0, 1.50);

    // Only index 1 in the output indices, but only index 0 has an override
    let output = format_price_diff_csv(&cards, &[1], &overrides);

    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 1); // header only
}
