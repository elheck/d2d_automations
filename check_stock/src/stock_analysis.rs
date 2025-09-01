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
    pub available_bins: HashMap<String, i32>,  // Location -> cards in bin
}

impl StockAnalysis {
    const BIN_CAPACITY: i32 = 60;  // Fixed bin capacity

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
        stats.available_bins = bin_counts.into_iter()
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
                return Some(format!("{}-{}-{}-{}", parts[0], parts[1], parts[2], parts[3]));
            }
        }
        None
    }
}

pub fn format_stock_analysis_with_sort(stats: &StockStats, sort_order: SortOrder) -> String {
    let mut output = String::new();
    
    output.push_str(&format!("Bin Analysis (Maximum Capacity per Bin: {} cards)\n", StockAnalysis::BIN_CAPACITY));
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
        output.push_str(&format!("{location}: {count} cards ({free_slots} slots free)\n"));
    }
    
    output
}