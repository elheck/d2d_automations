use d2d_automations::read_csv;
use std::env;
use std::process;
use std::fs::File;
use std::io::{self, BufRead};
use std::collections::HashMap;

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
    let mut total_price = 0.0;
    println!("AVAILABLE CARDS IN STOCK:");
    println!("========================\n");

    // Check each card in the decklist
    for (needed_quantity, card_name) in decklist {
        let matching_cards: Vec<_> = inventory.iter()
            .filter(|card| card.name.eq_ignore_ascii_case(&card_name))
            .collect();

        if !matching_cards.is_empty() {
            found_cards = true;
            
            // Group cards by set
            let mut cards_by_set: HashMap<String, Vec<&d2d_automations::Card>> = HashMap::new();
            for card in &matching_cards {
                let set_key = format!("{} ({})", &card.set, &card.set_code);
                cards_by_set.entry(set_key).or_default().push(card);
            }

            let mut remaining_needed = needed_quantity;
            let mut found_copies = Vec::new();
            let mut card_total_cost = 0.0;

            // Sort sets by price to use cheapest sets first
            let mut sets: Vec<_> = cards_by_set.iter().collect();
            sets.sort_by(|a, b| {
                let price_a = a.1[0].price.parse::<f64>().unwrap_or(f64::MAX);
                let price_b = b.1[0].price.parse::<f64>().unwrap_or(f64::MAX);
                price_a.partial_cmp(&price_b).unwrap()
            });

            // Calculate how many copies we can provide from each set
            for (set_name, cards) in sets {
                if remaining_needed <= 0 {
                    break;
                }

                let total_in_set: i32 = cards.iter()
                    .map(|card| card.quantity.parse::<i32>().unwrap_or(0))
                    .sum();

                if total_in_set > 0 {
                    let copies_from_set = remaining_needed.min(total_in_set);
                    if let Ok(price) = cards[0].price.parse::<f64>() {
                        card_total_cost += price * copies_from_set as f64;
                    }
                    found_copies.push((copies_from_set, set_name, cards[0]));
                    remaining_needed -= copies_from_set;
                }
            }

            if !found_copies.is_empty() {
                let total_found: i32 = found_copies.iter()
                    .map(|(qty, _, _)| qty)
                    .sum();

                println!("{} x {} (total: {:.2} €)", needed_quantity, card_name, card_total_cost);
                
                // Show copies from each set with their individual prices
                for (qty, set_name, card) in found_copies {
                    let location_info = card.location.as_ref()
                        .filter(|loc| !loc.trim().is_empty())
                        .map(|loc| format!(" [Location: {}]", loc))
                        .unwrap_or_default();
                    
                    println!("    {} {} [{}] from {}, {} condition - {:.2} €{}",
                        qty,
                        if qty == 1 { "copy" } else { "copies" },
                        card.language,
                        set_name,
                        card.condition,
                        card.price.parse::<f64>().unwrap_or(0.0),
                        location_info
                    );
                }

                if total_found < needed_quantity {
                    println!("    WARNING: Only {} of {} copies available!", 
                        total_found, needed_quantity);
                }

                total_price += card_total_cost;
                println!("");
            }
        }
    }

    if !found_cards {
        println!("No cards from your decklist were found in stock.");
    } else {
        println!("========================");
        println!("Total price for available cards: {:.2} €", total_price);
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
