use d2d_automations::{read_csv, read_decklist};
use std::env;
use std::process;

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

    // Check each card in the decklist
    for deck_entry in decklist {
        let matching_cards: Vec<_> = inventory.iter()
            .filter(|card| card.name.eq_ignore_ascii_case(&deck_entry.name))
            .collect();

        if matching_cards.is_empty() {
            println!("{} x {} - NOT IN STOCK", deck_entry.quantity, deck_entry.name);
            continue;
        }

        let total_available: i32 = matching_cards.iter()
            .map(|card| card.quantity.parse::<i32>().unwrap_or(0))
            .sum();

        println!("=== {} x {} ===", deck_entry.quantity, deck_entry.name);
        println!("Status: {} of {} copies available", total_available, deck_entry.quantity);

        // Show available copies with details
        for card in matching_cards {
            println!("  • {} copies [{}] {} €", 
                card.quantity,
                card.language,
                card.price
            );
        }

        if total_available < deck_entry.quantity {
            println!("  WARNING: Not enough copies in stock!");
        }
        println!("");
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
