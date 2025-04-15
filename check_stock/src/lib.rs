pub mod config;
pub mod errors;
pub mod utils;

pub use config::Config;
pub use errors::Error;

use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufRead};

#[derive(Debug, Deserialize)]
pub struct Card {
    #[serde(rename = "cardmarketId")]
    pub cardmarket_id: String,
    pub quantity: String,
    pub name: String,
    pub set: String,
    #[serde(rename = "setCode")]
    pub set_code: String,
    pub condition: String,
    pub language: String,
    pub price: String,
}

#[derive(Debug)]
pub struct DeckEntry {
    pub quantity: i32,
    pub name: String,
}

pub fn read_csv(path: &str) -> Result<Vec<Card>, Box<dyn std::error::Error>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let mut cards = Vec::new();

    for result in rdr.deserialize() {
        let card: Card = result?;
        cards.push(card);
    }

    Ok(cards)
}

fn parse_deck_line(line: &str) -> Option<(i32, String)> {
    let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
    if parts.len() != 2 {
        return None;
    }
    
    let quantity = parts[0].parse().ok()?;
    let name = parts[1].to_string();
    Some((quantity, name))
}

pub fn read_decklist(path: &str) -> Result<Vec<DeckEntry>, std::io::Error> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut deck = Vec::new();
    
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() || line.trim() == "Deck" {
            continue;
        }
        
        if let Some((quantity, name)) = parse_deck_line(&line) {
            deck.push(DeckEntry { quantity, name });
        }
    }
    
    Ok(deck)
}

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