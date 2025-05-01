use crate::models::Card;
use std::collections::HashMap;

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
            .filter(|(_, count)| count <= &(Self::BIN_CAPACITY - min_free_slots))
            .collect();

        stats
    }

    fn extract_bin_location(location: &str) -> Option<String> {
        let parts: Vec<&str> = location.split('-').collect();
        if parts.len() >= 4 {
            // Only take the first 4 parts (e.g., "A-0-1-4")
            if let Ok(_) = parts[3].parse::<i32>() {
                return Some(format!("{}-{}-{}-{}", parts[0], parts[1], parts[2], parts[3]));
            }
        }
        None
    }
}

pub fn format_stock_analysis(stats: &StockStats) -> String {
    let mut output = String::new();
    
    output.push_str(&format!("Available Bin Locations (Capacity: {}):\n", StockAnalysis::BIN_CAPACITY));
    output.push_str(&format!("(Showing bins with {} or more free slots)\n\n", 
        StockAnalysis::BIN_CAPACITY - stats.available_bins.iter().next().map(|(_, count)| *count).unwrap_or(0)));
    
    let mut available_locs: Vec<_> = stats.available_bins.iter().collect();
    available_locs.sort_by(|a, b| a.0.cmp(b.0));
    
    for (location, count) in available_locs {
        let free_slots = StockAnalysis::BIN_CAPACITY - count;
        output.push_str(&format!("{}: {} cards ({} slots free)\n", 
            location, count, free_slots));
    }
    
    output
}