use clap::Parser;
use std::process;
use std::fs::File;
use std::io::{self, Write, BufRead};
use std::collections::HashMap;
use std::path::Path;

mod gui;

/// A tool to check Magic: The Gathering card inventory against a wantslist
#[derive(Parser, Debug)]
#[command(version, about)]
pub struct Args {
    /// Path to the CSV inventory file
    #[arg(long = "inventory", short = 'i')]
    inventory_csv: Option<String>,

    /// Path to the wantslist file
    #[arg(short = 'w', long = "wants")]
    wantslist: Option<String>,

    /// Write output to a file instead of stdout
    /// The output file will have the same name as the input wantslist with "_in_stock" appended
    #[arg(short = 'o', long = "write-output")]
    write_output: bool,
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

fn write_to_output<W: Write>(writer: &mut W, content: &str) -> io::Result<()> {
    writer.write_all(content.as_bytes())
}

/// Parse a location code, converting the prefix letter to a number for sorting
fn parse_location_code(loc: &str) -> Vec<i32> {
    // Split the location at "-L0" to get only the main part
    let main_part = loc.split("-L0").next().unwrap_or(loc);
    
    main_part.split('-')
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                // Convert prefix letter to corresponding number for sorting
                match part.chars().next().unwrap_or('A') {
                    'A' => 1,
                    'B' => 2,
                    'C' => 3,
                    'D' => 4,
                    _ => 0,
                }
            } else {
                // Parse the numeric part
                part.parse::<i32>().unwrap_or(0)
            }
        })
        .collect()
}

pub fn run_with_args(args: &Args) -> Result<String, Box<dyn std::error::Error>> {
    // Read inventory
    let inventory = d2d_automations::read_csv(args.inventory_csv.as_ref()
        .ok_or("Inventory CSV path not provided")?)?;
    let inventory_message = format!("Loaded inventory with {} cards\n", inventory.len());

    // Read wantslist
    let wantslist = read_wantslist(args.wantslist.as_ref()
        .ok_or("Wantslist path not provided")?)?;
    let wantslist_message = format!("\nChecking availability for {} cards in wantslist...\n\n", wantslist.len());

    let header = "AVAILABLE CARDS IN STOCK:\n========================\n\n";

    let mut found_cards = false;
    let mut total_price = 0.0;
    let mut output_entries = Vec::new();

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

            // Create output entry for this card
            if !found_copies.is_empty() {
                let total_found: i32 = found_copies.iter()
                    .map(|(qty, _, _)| qty)
                    .sum();

                let mut card_output = Vec::new();
                card_output.push(format!("{} x {} (total: {:.2} €)\n", needed_quantity, card_name, card_total_cost));
                
                // Sort copies by location if available
                found_copies.sort_by(|(_, _, card_a), (_, _, card_b)| {
                    let loc_a = card_a.location.as_deref().unwrap_or("");
                    let loc_b = card_b.location.as_deref().unwrap_or("");
                    
                    if loc_a.is_empty() && loc_b.is_empty() {
                        std::cmp::Ordering::Equal
                    } else if loc_a.is_empty() {
                        std::cmp::Ordering::Greater
                    } else if loc_b.is_empty() {
                        std::cmp::Ordering::Less
                    } else {
                        let parts_a = parse_location_code(loc_a);
                        let parts_b = parse_location_code(loc_b);
                        parts_a.cmp(&parts_b)
                    }
                });

                // Show copies from each set with their individual prices
                for (qty, set_name, card) in &found_copies {
                    // Add location information if available
                    let location_info = card.location.as_ref()
                        .filter(|loc| !loc.trim().is_empty())
                        .map(|loc| format!(" [Location: {}]", loc))
                        .unwrap_or_default();
                    
                    card_output.push(format!("    {} {} [{}] from {}, {} condition - {:.2} €{}\n",
                        qty,
                        if *qty == 1 { "copy" } else { "copies" },
                        card.language,
                        set_name,
                        card.condition,
                        card.price.parse::<f64>().unwrap_or(0.0),
                        location_info
                    ));
                }

                // Show warning only if we have some cards but not enough
                if total_found < needed_quantity {
                    card_output.push(format!("    WARNING: Only {} of {} copies available!\n", 
                        total_found, needed_quantity));
                }

                card_output.push(String::from("\n"));
                
                // Add to output entries with sort key based on the first card's location
                let sort_key = found_copies.first()
                    .and_then(|(_, _, card)| card.location.as_ref())
                    .map(|loc| loc.to_string())
                    .unwrap_or_else(|| String::from(""));
                    
                output_entries.push((sort_key, card_output.join("")));
                total_price += card_total_cost;
            }
        }
    }

    // Sort the entire output by location if available
    output_entries.sort_by(|(loc_a, _), (loc_b, _)| {
        if loc_a.is_empty() && loc_b.is_empty() {
            std::cmp::Ordering::Equal
        } else if loc_a.is_empty() {
            std::cmp::Ordering::Greater
        } else if loc_b.is_empty() {
            std::cmp::Ordering::Less
        } else {
            let parts_a = parse_location_code(loc_a);
            let parts_b = parse_location_code(loc_b);
            parts_a.cmp(&parts_b)
        }
    });

    // Build final output
    let mut output = String::new();
    output.push_str(&inventory_message);
    output.push_str(&wantslist_message);
    output.push_str(header);

    // Add all card entries in sorted order
    for (_, entry) in output_entries {
        output.push_str(&entry);
    }

    // Display final results
    if !found_cards {
        output.push_str("No cards from your wantslist were found in stock.\n");
    } else {
        output.push_str("========================\n");
        output.push_str(&format!("Total price for available cards: {:.2} €\n", total_price));
    }

    if args.write_output {
        // Create output filename by appending "_in_stock" before the extension
        let wantslist_path = args.wantslist.as_ref().unwrap();
        let input_path = Path::new(wantslist_path);
        let stem = input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
        let extension = input_path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let output_filename = if extension.is_empty() {
            format!("{}_in_stock", stem)
        } else {
            format!("{}_in_stock.{}", stem, extension)
        };

        let mut output_file = File::create(&output_filename)?;
        write_to_output(&mut output_file, &output)?;
        println!("Results written to {}", output_filename);
    }

    Ok(output)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse_from(std::env::args_os());
    
    // If no inventory or wantslist provided, launch GUI
    if args.inventory_csv.is_none() || args.wantslist.is_none() {
        return Ok(gui::launch_gui()?);
    }
    
    let output = run_with_args(&args)?;
    
    if !args.write_output {
        print!("{}", output);
    }
    Ok(())
}

/// Program entry point
fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
