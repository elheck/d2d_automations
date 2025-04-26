use std::io::{self, BufRead};
use std::fs::File;
use crate::models::{Card, WantsEntry};

pub fn read_csv(path: &str) -> Result<Vec<Card>, Box<dyn std::error::Error>> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_path(path)?;
    
    let mut cards = Vec::new();

    for result in rdr.deserialize() {
        let card: Card = result?;
        if !card.price.trim().is_empty() && !card.quantity.trim().is_empty() {
            cards.push(card);
        }
    }

    Ok(cards)
}

fn parse_wants_line(line: &str) -> Option<(i32, String)> {
    let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
    if parts.len() != 2 {
        return None;
    }
    
    let quantity = parts[0].parse().ok()?;
    let name = parts[1].to_string();
    Some((quantity, name))
}

pub fn read_wantslist(path: &str) -> Result<Vec<WantsEntry>, io::Error> {
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);
    let mut wants = Vec::new();
    
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() || line.trim() == "Deck" {
            continue;
        }
        
        if let Some((quantity, name)) = parse_wants_line(&line) {
            wants.push(WantsEntry { quantity, name });
        }
    }
    
    Ok(wants)
}