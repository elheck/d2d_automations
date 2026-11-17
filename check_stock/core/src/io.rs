use crate::category::detect_category;
use crate::models::{Card, WantsEntry};
use crate::wantslist::parse_wantslist;
use log::info;
use std::io;

/// An inventory CSV together with the product category it belongs to.
///
/// Cardmarket exports one report per category; the category must travel with
/// the rows so a sync only claims ownership of its own slice of the database
/// (see [`crate::inventory_db::InventoryDb::sync_inventory`]).
#[derive(Debug, Clone)]
pub struct InventoryCsv {
    pub cards: Vec<Card>,
    pub category: String,
}

/// Reads an inventory CSV and determines which product category it holds.
///
/// Use this for anything that writes to the inventory DB; [`read_csv`] remains
/// available for read-only consumers that don't care about the category.
pub fn read_csv_with_category(path: &str) -> Result<InventoryCsv, Box<dyn std::error::Error>> {
    let cards = read_csv(path)?;
    let category = detect_category(path, &cards);
    Ok(InventoryCsv { cards, category })
}

pub fn read_csv(path: &str) -> Result<Vec<Card>, Box<dyn std::error::Error>> {
    info!("Reading inventory CSV from: {}", path);
    let file = std::fs::File::open(path)?;
    Ok(read_cards(file, None)?)
}

/// Why an inventory CSV could not be read.
#[derive(Debug)]
pub enum CsvReadError {
    /// Malformed CSV, a missing column or invalid UTF-8.
    Csv(csv::Error),
    /// More rows than the caller allows (counted before rows are skipped).
    TooManyRows { max: usize },
}

impl CsvReadError {
    /// Line of the offending row, if known.
    pub fn line(&self) -> Option<u64> {
        match self {
            CsvReadError::Csv(e) => e.position().map(|p| p.line()),
            CsvReadError::TooManyRows { .. } => None,
        }
    }
}

impl std::fmt::Display for CsvReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CsvReadError::Csv(e) => write!(f, "{e}"),
            CsvReadError::TooManyRows { max } => write!(f, "more than {max} rows"),
        }
    }
}

impl std::error::Error for CsvReadError {}

/// Parses an inventory CSV that is already in memory (an upload), with the
/// same rules as [`read_csv`]; stops with [`CsvReadError::TooManyRows`] after
/// `max_rows` rows, so a huge upload is never fully turned into cards.
pub fn parse_csv(data: &[u8], max_rows: usize) -> Result<Vec<Card>, CsvReadError> {
    read_cards(data, Some(max_rows))
}

/// Reads inventory rows: rows without price or quantity, and rows with
/// quantity 0, are skipped.
fn read_cards<R: io::Read>(source: R, max_rows: Option<usize>) -> Result<Vec<Card>, CsvReadError> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(source);

    let mut cards = Vec::new();
    let mut skipped_empty = 0;
    let mut skipped_zero = 0;

    for (index, result) in rdr.deserialize().enumerate() {
        if max_rows.is_some_and(|max| index >= max) {
            return Err(CsvReadError::TooManyRows {
                max: max_rows.unwrap_or_default(),
            });
        }
        let card: Card = result.map_err(CsvReadError::Csv)?;
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
