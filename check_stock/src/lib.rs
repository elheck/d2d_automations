// Import required modules and functionality
pub mod config;
pub mod errors;
pub mod utils;

pub use config::Config;
pub use errors::Error;

use serde::Deserialize;
use std::collections::HashMap;
use std::io::BufRead;

/// Represents a card from the inventory CSV file
/// Contains all possible fields that might be present in different CSV formats
#[derive(Debug, Deserialize)]
pub struct Card {
    #[serde(rename = "cardmarketId")]
    pub cardmarket_id: String,
    pub quantity: String,
    pub name: String,
    pub set: String,
    #[serde(rename = "setCode")]
    pub set_code: String,
    pub cn: String,
    pub condition: String,
    pub language: String,
    #[serde(rename = "isFoil")]
    pub is_foil: String,
    #[serde(rename = "isPlayset")]
    pub is_playset: String,
    #[serde(rename = "isSigned")]
    pub is_signed: String,
    pub price: String,
    pub comment: String,
    pub location: Option<String>,  // Optional field for storage location
    #[serde(rename = "nameDE")]
    pub name_de: String,
    #[serde(rename = "nameES")]
    pub name_es: String,
    #[serde(rename = "nameFR")]
    pub name_fr: String,
    #[serde(rename = "nameIT")]
    pub name_it: String,
    pub rarity: String,
    #[serde(rename = "listedAt")]
    pub listed_at: String,
}

/// Represents a card entry in a decklist
#[derive(Debug)]
pub struct DeckEntry {
    pub quantity: i32,
    pub name: String,
}

/// Reads and parses a CSV file containing card inventory
/// Returns a vector of Card structs
pub fn read_csv(path: &str) -> Result<Vec<Card>, Box<dyn std::error::Error>> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)  // Allow varying number of columns
        .trim(csv::Trim::All)  // Trim whitespace from all fields
        .from_path(path)?;
    
    let mut cards = Vec::new();

    // Parse each row and only include cards with valid price and quantity
    for result in rdr.deserialize() {
        let card: Card = result?;
        if !card.price.trim().is_empty() && !card.quantity.trim().is_empty() {
            cards.push(card);
        }
    }

    Ok(cards)
}

/// Parses a single line from a decklist file
/// Returns a tuple of (quantity, card_name) if valid
fn parse_deck_line(line: &str) -> Option<(i32, String)> {
    let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
    if parts.len() != 2 {
        return None;
    }
    
    let quantity = parts[0].parse().ok()?;
    let name = parts[1].to_string();
    Some((quantity, name))
}

/// Reads and parses a decklist file
/// Returns a vector of DeckEntry structs
pub fn read_decklist(path: &str) -> Result<Vec<DeckEntry>, std::io::Error> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut deck = Vec::new();
    
    for line in reader.lines() {
        let line = line?;
        // Skip empty lines and "Deck" header
        if line.trim().is_empty() || line.trim() == "Deck" {
            continue;
        }
        
        if let Some((quantity, name)) = parse_deck_line(&line) {
            deck.push(DeckEntry { quantity, name });
        }
    }
    
    Ok(deck)
}

/// Checks inventory stock against a decklist
/// Returns a HashMap mapping card names to available copies
pub fn check_stock<'a>(deck: &[DeckEntry], inventory: &'a [Card]) -> HashMap<String, Vec<&'a Card>> {
    let mut results = HashMap::new();
    
    for entry in deck {
        let matching_cards: Vec<&Card> = inventory
            .iter()
            .filter(|card| card.name.eq_ignore_ascii_case(&entry.name))
            .collect();
            
        if !matching_cards.is_empty() {
            results.insert(entry.name.clone(), matching_cards);
        }
    }
    
    results
}

// Initialize logging or any other setup code here
pub fn init() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}