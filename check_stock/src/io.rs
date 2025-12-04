use crate::models::{Card, WantsEntry};
use log::{debug, info, warn};
use std::fs::File;
use std::io::{self, BufRead};

pub fn read_csv(path: &str) -> Result<Vec<Card>, Box<dyn std::error::Error>> {
    info!("Reading inventory CSV from: {}", path);

    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_path(path)?;

    let mut cards = Vec::new();
    let mut skipped = 0;

    for result in rdr.deserialize() {
        let card: Card = result?;
        if !card.price.trim().is_empty() && !card.quantity.trim().is_empty() {
            cards.push(card);
        } else {
            skipped += 1;
        }
    }

    info!(
        "Loaded {} cards from inventory (skipped {} with empty price/quantity)",
        cards.len(),
        skipped
    );
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
    info!("Reading wantslist from: {}", path);

    let file = File::open(path)?;
    let reader = io::BufReader::new(file);
    let mut wants = Vec::new();
    let mut skipped_lines = 0;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() || line.trim() == "Deck" {
            continue;
        }

        if let Some((quantity, name)) = parse_wants_line(&line) {
            debug!("Parsed want: {} x {}", quantity, name);
            wants.push(WantsEntry { quantity, name });
        } else {
            warn!("Could not parse wantslist line: {}", line);
            skipped_lines += 1;
        }
    }

    info!(
        "Loaded {} entries from wantslist (skipped {} unparseable lines)",
        wants.len(),
        skipped_lines
    );
    Ok(wants)
}
