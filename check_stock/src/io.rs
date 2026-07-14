use crate::models::{Card, WantsEntry};
use crate::wantslist::parse_wantslist;
use log::info;
use std::io;

pub fn read_csv(path: &str) -> Result<Vec<Card>, Box<dyn std::error::Error>> {
    info!("Reading inventory CSV from: {}", path);

    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_path(path)?;

    let mut cards = Vec::new();
    let mut skipped_empty = 0;
    let mut skipped_zero = 0;

    for result in rdr.deserialize() {
        let card: Card = result?;
        if card.price.trim().is_empty() || card.quantity.trim().is_empty() {
            skipped_empty += 1;
            continue;
        }
        // The inventory-report CSV emits rows with quantity 0 both as summary
        // placeholders and as "last-known shelf" entries for sold-out variants.
        // Neither represents real inventory and they'd only dilute the DB sync's
        // representative-picking / zeroing logic, so drop them at the read layer.
        if card.quantity.trim().parse::<i64>().ok() == Some(0) {
            skipped_zero += 1;
            continue;
        }
        cards.push(card);
    }

    info!(
        "Loaded {} cards from inventory (skipped {} with empty price/quantity, {} with quantity 0)",
        cards.len(),
        skipped_empty,
        skipped_zero
    );
    Ok(cards)
}

/// Loads a wantslist from either a **deck URL** or a **file path**.
///
/// If `input` is a recognised Moxfield or Archidekt deck link, the deck is
/// fetched over the network ([`crate::deck_fetch`]); otherwise `input` is treated
/// as a file path and read via [`read_wantslist`]. Both paths return the same
/// merged [`WantsEntry`] list, so the caller is agnostic to the source.
pub fn load_wantslist(input: &str) -> Result<Vec<WantsEntry>, String> {
    match crate::deck_fetch::parse_deck_url(input) {
        Some(source) => crate::deck_fetch::fetch_deck(&source),
        None => read_wantslist(input).map_err(|e| e.to_string()),
    }
}

/// Reads a wantslist / decklist file and parses it via [`parse_wantslist`],
/// which understands the common community export formats (plain, Arena, MTGO,
/// Moxfield, Archidekt, MTGGoldfish). Duplicate card names are merged.
pub fn read_wantslist(path: &str) -> Result<Vec<WantsEntry>, io::Error> {
    info!("Reading wantslist from: {}", path);

    let content = std::fs::read_to_string(path)?;
    let parsed = parse_wantslist(&content);

    for line in &parsed.unparseable {
        log::warn!("Could not parse wantslist line: {}", line);
    }
    info!(
        "Loaded {} entries from wantslist (skipped {} unparseable lines)",
        parsed.entries.len(),
        parsed.unparseable.len()
    );
    Ok(parsed.entries)
}
