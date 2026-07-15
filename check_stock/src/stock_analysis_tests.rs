//! Tests for stock_analysis.

use super::*;

fn create_card_at_location(location: &str, quantity: i32) -> Card {
    Card {
        quantity: quantity.to_string(),
        location: Some(location.to_string()),
        ..Card::test_default()
    }
}

fn create_card_without_location(quantity: i32) -> Card {
    Card {
        quantity: quantity.to_string(),
        ..Card::test_default()
    }
}

// ==================== StockAnalysis Tests ====================

#[test]
fn test_stock_analysis_new() {
    let cards = vec![create_card_at_location("A-0-1-1", 10)];
    let analysis = StockAnalysis::new(cards);
    assert!(!analysis.cards.is_empty());
}

#[test]
fn test_analyze_with_free_slots_single_bin() {
    let cards = vec![create_card_at_location("A-0-1-1", 10)];
    let analysis = StockAnalysis::new(cards);
    let stats = analysis.analyze_with_free_slots(5);

    // Bin has 10 cards, 50 free slots, should be included (min_free_slots = 5)
    assert_eq!(stats.available_bins.len(), 1);
    assert_eq!(*stats.available_bins.get("A-0-1-1").unwrap(), 10);
}

#[test]
fn test_analyze_with_free_slots_excludes_full_bins() {
    let cards = vec![create_card_at_location("A-0-1-1", 58)];
    let analysis = StockAnalysis::new(cards);
    let stats = analysis.analyze_with_free_slots(5);

    // Bin has 58 cards, only 2 free slots, should be excluded (min_free_slots = 5)
    assert!(stats.available_bins.is_empty());
}

#[test]
fn test_analyze_with_free_slots_multiple_cards_same_bin() {
    let cards = vec![
        create_card_at_location("A-0-1-1", 10),
        create_card_at_location("A-0-1-1", 15),
        create_card_at_location("A-0-1-1", 5),
    ];
    let analysis = StockAnalysis::new(cards);
    let stats = analysis.analyze_with_free_slots(5);

    // Total: 30 cards, 30 free slots
    assert_eq!(*stats.available_bins.get("A-0-1-1").unwrap(), 30);
}

#[test]
fn test_analyze_with_free_slots_multiple_bins() {
    let cards = vec![
        create_card_at_location("A-0-1-1", 10),
        create_card_at_location("A-0-1-2", 20),
        create_card_at_location("B-1-2-3", 30),
    ];
    let analysis = StockAnalysis::new(cards);
    let stats = analysis.analyze_with_free_slots(5);

    assert_eq!(stats.available_bins.len(), 3);
    assert_eq!(*stats.available_bins.get("A-0-1-1").unwrap(), 10);
    assert_eq!(*stats.available_bins.get("A-0-1-2").unwrap(), 20);
    assert_eq!(*stats.available_bins.get("B-1-2-3").unwrap(), 30);
}

#[test]
fn test_analyze_with_free_slots_ignores_cards_without_location() {
    let cards = vec![
        create_card_at_location("A-0-1-1", 10),
        create_card_without_location(100),
    ];
    let analysis = StockAnalysis::new(cards);
    let stats = analysis.analyze_with_free_slots(5);

    // Only the card with location should be counted
    assert_eq!(stats.available_bins.len(), 1);
}

#[test]
fn test_analyze_with_free_slots_ignores_l0_suffix() {
    let cards = vec![
        create_card_at_location("A-0-1-1-L0-R", 10),
        create_card_at_location("A-0-1-1-L0-L", 15),
    ];
    let analysis = StockAnalysis::new(cards);
    let stats = analysis.analyze_with_free_slots(5);

    // Both should be grouped under A-0-1-1
    assert_eq!(stats.available_bins.len(), 1);
    assert_eq!(*stats.available_bins.get("A-0-1-1").unwrap(), 25);
}

#[test]
fn test_analyze_with_free_slots_empty_inventory() {
    let cards: Vec<Card> = vec![];
    let analysis = StockAnalysis::new(cards);
    let stats = analysis.analyze_with_free_slots(5);

    assert!(stats.available_bins.is_empty());
}

#[test]
fn test_analyze_with_free_slots_empty_location_string() {
    let mut card = create_card_at_location("A-0-1-1", 10);
    card.location = Some("".to_string());

    let cards = vec![card];
    let analysis = StockAnalysis::new(cards);
    let stats = analysis.analyze_with_free_slots(5);

    // Empty location string should be ignored
    assert!(stats.available_bins.is_empty());
}

// ==================== extract_bin_location Tests ====================

#[test]
fn test_extract_bin_location_valid() {
    assert_eq!(
        StockAnalysis::extract_bin_location("A-0-1-4"),
        Some("A-0-1-4".to_string())
    );
}

#[test]
fn test_extract_bin_location_with_suffix() {
    assert_eq!(
        StockAnalysis::extract_bin_location("A-0-1-4-L0-R"),
        Some("A-0-1-4".to_string())
    );
}

#[test]
fn test_extract_bin_location_too_few_parts() {
    assert_eq!(StockAnalysis::extract_bin_location("A-0-1"), None);
}

#[test]
fn test_extract_bin_location_invalid_fourth_part() {
    assert_eq!(StockAnalysis::extract_bin_location("A-0-1-X"), None);
}

// ==================== format_stock_analysis_with_sort Tests ====================

#[test]
fn test_format_stock_analysis_empty() {
    let stats = StockStats::default();
    let output = format_stock_analysis_with_sort(&stats, SortOrder::ByLocation);

    assert!(output.contains("Bin Analysis"));
    assert!(output.contains("Maximum Capacity per Bin: 60"));
}

#[test]
fn test_format_stock_analysis_by_location() {
    let mut stats = StockStats::default();
    stats.available_bins.insert("B-1-1-1".to_string(), 20);
    stats.available_bins.insert("A-0-0-1".to_string(), 10);

    let output = format_stock_analysis_with_sort(&stats, SortOrder::ByLocation);

    // A should come before B when sorted by location
    let a_pos = output.find("A-0-0-1").unwrap();
    let b_pos = output.find("B-1-1-1").unwrap();
    assert!(a_pos < b_pos);
}

#[test]
fn test_format_stock_analysis_by_free_slots() {
    let mut stats = StockStats::default();
    stats.available_bins.insert("A-0-0-1".to_string(), 50); // 10 free
    stats.available_bins.insert("B-1-1-1".to_string(), 10); // 50 free

    let output = format_stock_analysis_with_sort(&stats, SortOrder::ByFreeSlots);

    // B should come first (more free slots)
    let a_pos = output.find("A-0-0-1").unwrap();
    let b_pos = output.find("B-1-1-1").unwrap();
    assert!(b_pos < a_pos);
}

#[test]
fn test_format_stock_analysis_shows_free_slots() {
    let mut stats = StockStats::default();
    stats.available_bins.insert("A-0-0-1".to_string(), 20);

    let output = format_stock_analysis_with_sort(&stats, SortOrder::ByLocation);

    assert!(output.contains("20 cards"));
    assert!(output.contains("40 slots free"));
}
