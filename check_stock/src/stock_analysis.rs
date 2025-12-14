use crate::models::Card;
use std::collections::HashMap;

#[derive(PartialEq, Clone, Copy)]
pub enum SortOrder {
    ByFreeSlots,
    ByLocation,
}

pub struct StockAnalysis {
    cards: Vec<Card>,
}

#[derive(Default)]
pub struct StockStats {
    pub available_bins: HashMap<String, i32>, // Location -> cards in bin
}

impl StockAnalysis {
    const BIN_CAPACITY: i32 = 60; // Fixed bin capacity

    pub fn new(cards: Vec<Card>) -> Self {
        Self { cards }
    }

    pub fn analyze_with_free_slots(&self, min_free_slots: i32) -> StockStats {
        let mut stats = StockStats::default();

        // Collect all cards by their bin location
        let mut bin_counts: HashMap<String, i32> = HashMap::new();

        for card in &self.cards {
            if let Some(loc) = &card.location {
                if !loc.trim().is_empty() {
                    // Extract and count by base location (ignoring L0, R, etc.)
                    if let Some(bin_loc) = Self::extract_bin_location(loc) {
                        let quantity = card.quantity.parse::<i32>().unwrap_or(0);
                        *bin_counts.entry(bin_loc).or_insert(0) += quantity;
                    }
                }
            }
        }

        // Store bins that have the required number of free slots or more
        stats.available_bins = bin_counts
            .into_iter()
            .filter(|(_, count)| {
                let free_slots = Self::BIN_CAPACITY - count;
                free_slots >= min_free_slots
            })
            .collect();

        stats
    }

    fn extract_bin_location(location: &str) -> Option<String> {
        let parts: Vec<&str> = location.split('-').collect();
        if parts.len() >= 4 {
            // Only take the first 4 parts (e.g., "A-0-1-4")
            if parts[3].parse::<i32>().is_ok() {
                return Some(format!(
                    "{}-{}-{}-{}",
                    parts[0], parts[1], parts[2], parts[3]
                ));
            }
        }
        None
    }
}

pub fn format_stock_analysis_with_sort(stats: &StockStats, sort_order: SortOrder) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "Bin Analysis (Maximum Capacity per Bin: {} cards)\n",
        StockAnalysis::BIN_CAPACITY
    ));
    output.push_str("-----------------------------------------------\n\n");

    let mut available_locs: Vec<_> = stats.available_bins.iter().collect();

    match sort_order {
        SortOrder::ByFreeSlots => {
            available_locs.sort_by(|a, b| {
                let free_slots_a = StockAnalysis::BIN_CAPACITY - a.1;
                let free_slots_b = StockAnalysis::BIN_CAPACITY - b.1;
                // Sort by free slots (descending), then by location
                free_slots_b.cmp(&free_slots_a).then(a.0.cmp(b.0))
            });
        }
        SortOrder::ByLocation => {
            available_locs.sort_by(|a, b| a.0.cmp(b.0));
        }
    }

    for (location, count) in available_locs {
        let free_slots = StockAnalysis::BIN_CAPACITY - count;
        output.push_str(&format!(
            "{location}: {count} cards ({free_slots} slots free)\n"
        ));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a test card with a specific location and quantity
    fn create_card_at_location(location: &str, quantity: i32) -> Card {
        Card {
            cardmarket_id: "12345".to_string(),
            quantity: quantity.to_string(),
            name: "Test Card".to_string(),
            set: "Test Set".to_string(),
            set_code: "TST".to_string(),
            cn: "1".to_string(),
            condition: "NM".to_string(),
            language: "English".to_string(),
            is_foil: "false".to_string(),
            is_playset: None,
            is_signed: "false".to_string(),
            price: "1.00".to_string(),
            comment: "".to_string(),
            location: Some(location.to_string()),
            name_de: "".to_string(),
            name_es: "".to_string(),
            name_fr: "".to_string(),
            name_it: "".to_string(),
            rarity: "common".to_string(),
            listed_at: "2024-01-01".to_string(),
        }
    }

    fn create_card_without_location(quantity: i32) -> Card {
        Card {
            cardmarket_id: "12345".to_string(),
            quantity: quantity.to_string(),
            name: "Test Card".to_string(),
            set: "Test Set".to_string(),
            set_code: "TST".to_string(),
            cn: "1".to_string(),
            condition: "NM".to_string(),
            language: "English".to_string(),
            is_foil: "false".to_string(),
            is_playset: None,
            is_signed: "false".to_string(),
            price: "1.00".to_string(),
            comment: "".to_string(),
            location: None,
            name_de: "".to_string(),
            name_es: "".to_string(),
            name_fr: "".to_string(),
            name_it: "".to_string(),
            rarity: "common".to_string(),
            listed_at: "2024-01-01".to_string(),
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
}
