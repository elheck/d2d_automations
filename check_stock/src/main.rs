use clap::Parser;
use d2d_automations::read_csv;
use std::process;
use std::fs::File;
use std::io::{self, BufRead};
use std::collections::HashMap;

/// A tool to check Magic: The Gathering card inventory against a wantslist
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Path to the CSV inventory file
    inventory_csv: String,

    /// Path to the wantslist file
    #[arg(short = 'w', long = "wants")]
    wantslist: String,
}

/// Parses a single line from a wantslist
/// Format: "{quantity} {card_name}"
/// Returns None if the line format is invalid
fn parse_wants_line(line: &str) -> Option<(i32, String)> {
    let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
    if parts.len() != 2 {
        return None;
    }
    
    let quantity = parts[0].parse().ok()?;
    let name = parts[1].to_string();
    Some((quantity, name))
}

/// Reads a wantslist from a file
/// Skips empty lines and the "Deck" header
/// Returns a vector of (quantity, card_name) tuples
fn read_wantslist(path: &str) -> Result<Vec<(i32, String)>, io::Error> {
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);
    let mut wants = Vec::new();
    
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() || line.trim() == "Deck" {
            continue;
        }
        
        if let Some((quantity, name)) = parse_wants_line(&line) {
            wants.push((quantity, name));
        }
    }
    
    Ok(wants)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Read inventory
    let inventory = read_csv(&args.inventory_csv)?;
    println!("Loaded inventory with {} cards", inventory.len());

    // Read wantslist
    let wantslist = read_wantslist(&args.wantslist)?;
    println!("\nChecking availability for {} cards in wantslist...\n", wantslist.len());

    let mut found_cards = false;
    let mut total_price = 0.0;
    println!("AVAILABLE CARDS IN STOCK:");
    println!("========================\n");

    // Process each card in the wantslist
    for (needed_quantity, card_name) in wantslist {
        // Find all matching cards in inventory
        let matching_cards: Vec<_> = inventory.iter()
            .filter(|card| card.name.eq_ignore_ascii_case(&card_name))
            .collect();

        if !matching_cards.is_empty() {
            found_cards = true;
            
            // Group cards by set for better organization
            let mut cards_by_set: HashMap<String, Vec<&d2d_automations::Card>> = HashMap::new();
            for card in &matching_cards {
                let set_key = format!("{} ({})", &card.set, &card.set_code);
                cards_by_set.entry(set_key).or_default().push(card);
            }

            let mut remaining_needed = needed_quantity;
            let mut found_copies = Vec::new();
            let mut card_total_cost = 0.0;

            // Sort sets by price to prioritize cheaper versions
            let mut sets: Vec<_> = cards_by_set.iter().collect();
            sets.sort_by(|a, b| {
                let price_a = a.1[0].price.parse::<f64>().unwrap_or(f64::MAX);
                let price_b = b.1[0].price.parse::<f64>().unwrap_or(f64::MAX);
                price_a.partial_cmp(&price_b).unwrap()
            });

            // Calculate available copies from each set
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

            // Display results for this card
            if !found_copies.is_empty() {
                let total_found: i32 = found_copies.iter()
                    .map(|(qty, _, _)| qty)
                    .sum();

                println!("{} x {} (total: {:.2} €)", needed_quantity, card_name, card_total_cost);
                
                // Show copies from each set with their individual prices
                for (qty, set_name, card) in found_copies {
                    // Add location information if available
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

                // Show warning only if we have some cards but not enough
                if total_found < needed_quantity {
                    println!("    WARNING: Only {} of {} copies available!", 
                        total_found, needed_quantity);
                }

                total_price += card_total_cost;
                println!("");
            }
        }
    }

    // Display final results
    if !found_cards {
        println!("No cards from your wantslist were found in stock.");
    } else {
        println!("========================");
        println!("Total price for available cards: {:.2} €", total_price);
    }

    Ok(())
}

/// Program entry point
/// Handles any errors from the run function
fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
