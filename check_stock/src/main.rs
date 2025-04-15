use d2d_automations::read_csv;
use std::env;
use std::process;
use std::fs::File;
use std::io::{self, BufRead};

fn parse_deck_line(line: &str) -> Option<(i32, String)> {
    let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
    if parts.len() != 2 {
        return None;
    }
    
    let quantity = parts[0].parse().ok()?;
    let name = parts[1].to_string();
    Some((quantity, name))
}

fn read_decklist(path: &str) -> Result<Vec<(i32, String)>, io::Error> {
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);
    let mut deck = Vec::new();
    
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() || line.trim() == "Deck" {
            continue;
        }
        
        if let Some((quantity, name)) = parse_deck_line(&line) {
            deck.push((quantity, name));
        }
    }
    
    Ok(deck)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() != 3 {
        eprintln!("Usage: {} <inventory.csv> <decklist.txt>", args[0]);
        process::exit(1);
    }

    let inventory_path = &args[1];
    let decklist_path = &args[2];

    // Read inventory
    let inventory = read_csv(inventory_path)?;
    println!("Loaded inventory with {} cards", inventory.len());

    // Read decklist
    let decklist = read_decklist(decklist_path)?;
    println!("\nChecking availability for {} cards in decklist...\n", decklist.len());

    let mut found_cards = false;
    println!("AVAILABLE CARDS:");
    println!("================\n");

    // Check each card in the decklist
    for (needed_quantity, card_name) in decklist {
        let matching_cards: Vec<_> = inventory.iter()
            .filter(|card| card.name.eq_ignore_ascii_case(&card_name))
            .collect();

        let total_available: i32 = matching_cards.iter()
            .map(|card| card.quantity.parse::<i32>().unwrap_or(0))
            .sum();

        // Only show cards that are in stock and have enough quantity
        if !matching_cards.is_empty() && total_available >= needed_quantity {
            found_cards = true;
            println!("✓ {} x {}", needed_quantity, card_name);
            println!("  Available copies:");
            
            // Show available copies with details
            for card in matching_cards {
                println!("    • {} copies [{}] from {} ({}) - {} condition - {} €", 
                    card.quantity,
                    card.language,
                    card.set,
                    card.set_code,
                    card.condition,
                    card.price
                );
            }
            println!("");
        }
    }

    if !found_cards {
        println!("No cards from your decklist were found in stock.");
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
