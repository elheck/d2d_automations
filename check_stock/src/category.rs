//! Product category detection for Cardmarket inventory exports.
//!
//! Cardmarket exports one inventory report *per game/category*. Each report is
//! complete for its own category but says nothing about the others, so a sync
//! must only ever claim ownership of the rows in its own category — otherwise
//! loading `inventory-report-Generic.csv` would zero out every Magic card (see
//! [`crate::inventory_db::sync_inventory`]).
//!
//! Product IDs are namespaced per category too: a sleeve and a Magic card can
//! both carry `cardmarketId` 716833 while being entirely different products.
//! The category is therefore part of the article key, not just a filter.

use crate::models::Card;
use std::path::Path;

/// The category all pre-category-tracking rows are attributed to.
///
/// Everything synced before category scoping existed was a Magic export, so the
/// schema migration backfills this value (see `MIGRATION_ADD_CATEGORY`).
pub const DEFAULT_CATEGORY: &str = "Magic";

/// The category of accessories (sleeves, deck boxes, playmats).
pub const GENERIC_CATEGORY: &str = "Generic";

/// Cardmarket's numeric game ID for Magic, as it appears in `inventory-report-45.csv`.
const MAGIC_GAME_ID: &str = "45";

/// Maps Cardmarket's numeric game IDs to the names we store.
///
/// Only IDs we have actually seen in exports are listed. Any other number is
/// treated as carrying no category information (see [`category_from_filename`]).
const GAME_IDS: &[(&str, &str)] = &[
    (MAGIC_GAME_ID, "Magic"),
    ("1", "Magic"),
    ("2", "YuGiOh"),
    ("3", "Pokemon"),
    ("6", "FleshAndBlood"),
    ("7", "Digimon"),
    ("8", "DragonBallSuper"),
    ("9", "OnePiece"),
    ("10", "Lorcana"),
    ("15", "StarWarsUnlimited"),
];

/// Determines the category of an inventory export.
///
/// The filename is authoritative when it carries a recognisable
/// `inventory-report-<suffix>` marker; otherwise the rows themselves are
/// inspected. Returns [`DEFAULT_CATEGORY`] when neither source is conclusive,
/// which keeps the behaviour of existing Magic-only setups unchanged.
pub fn detect_category(path: &str, cards: &[Card]) -> String {
    if let Some(category) = category_from_filename(path) {
        log::info!("Inventory category '{category}' detected from filename '{path}'");
        return category;
    }
    let category = category_from_rows(cards).unwrap_or_else(|| DEFAULT_CATEGORY.to_string());
    log::info!("Inventory category '{category}' inferred from CSV contents of '{path}'");
    category
}

/// Extracts the category from an `inventory-report-<suffix>.csv` filename.
///
/// Known numeric suffixes are Cardmarket game IDs and are mapped to names;
/// textual suffixes (`Generic`) are used as-is, after [`canonical_category`]
/// has removed any duplicate-download decoration. Returns `None` for filenames
/// that carry no usable suffix — including numeric suffixes we have no name for
/// — leaving the decision to content inspection.
fn category_from_filename(path: &str) -> Option<String> {
    let stem = Path::new(path).file_stem()?.to_str()?;
    let suffix = strip_duplicate_marker(strip_report_prefix(stem)?);
    if suffix.is_empty() {
        return None;
    }

    if let Some((_, name)) = GAME_IDS
        .iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(suffix))
    {
        return Some((*name).to_string());
    }

    // A number we have no name for is not evidence of a separate game:
    // Cardmarket also names reports `inventory-report-5173.csv` for a plain
    // Magic export. Giving it a scope of its own (`Game5173`) duplicated the
    // whole inventory, so the rows decide instead.
    if suffix.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    Some(normalize_name(suffix))
}

/// Maps a stored category name onto the name the current detection rules would
/// produce, or `None` when it is already canonical.
///
/// Older versions scoped `inventory-report-Magic (1).csv` as `Magic (1)` and
/// `inventory-report-5173.csv` as `Game5173`, splitting one inventory across
/// several categories. `has_card_metadata` says whether the category's rows
/// look like cards (see [`category_from_rows`]); it settles the legacy
/// `Game<digits>` names, which carry no information of their own.
pub fn canonical_category(name: &str, has_card_metadata: bool) -> Option<String> {
    let stripped = strip_duplicate_marker(name);
    let legacy_game_id = stripped
        .strip_prefix("Game")
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()));

    let canonical = match legacy_game_id {
        Some(id) => match GAME_IDS.iter().find(|(known, _)| *known == id) {
            Some((_, known_name)) => (*known_name).to_string(),
            None if has_card_metadata => DEFAULT_CATEGORY.to_string(),
            None => GENERIC_CATEGORY.to_string(),
        },
        None => normalize_name(stripped),
    };
    (canonical != name && !canonical.is_empty()).then_some(canonical)
}

/// Removes the decoration browsers and file managers add to a repeated
/// download — `Magic (1)`, `Magic(2)`, `Magic - Copy`, `Magic copy` — so every
/// copy of an export lands in the same category.
fn strip_duplicate_marker(name: &str) -> &str {
    let mut name = name.trim();
    loop {
        let before = name;
        if let Some(open) = name.rfind('(') {
            let inner = &name[open + 1..];
            if let Some(digits) = inner.strip_suffix(')') {
                if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                    name = name[..open].trim_end();
                }
            }
        }
        let lower = name.to_lowercase();
        for marker in [" - copy", " copy"] {
            if lower.ends_with(marker) {
                name = name[..name.len() - marker.len()].trim_end();
                break;
            }
        }
        if name == before {
            return name;
        }
    }
}

/// Strips a leading `inventory-report-` / `inventory_report_` marker (in any
/// case) and returns the remaining suffix.
fn strip_report_prefix(stem: &str) -> Option<&str> {
    let lower = stem.to_lowercase();
    let marker_len = ["inventory-report-", "inventory_report_"]
        .iter()
        .find_map(|marker| lower.starts_with(marker).then_some(marker.len()))?;
    Some(&stem[marker_len..])
}

/// Title-cases a free-form suffix so `generic` and `GENERIC` map to one scope.
fn normalize_name(suffix: &str) -> String {
    let mut chars = suffix.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}

/// Infers the category from the rows themselves, for exports whose filename was
/// renamed or carries no suffix.
///
/// Generic products (sleeves, deck boxes, playmats) are accessories rather than
/// cards: they have no expansion, no collector number and no rarity. Real card
/// exports always populate at least one of those on some row. `None` means the
/// rows were inconclusive (e.g. an empty CSV).
fn category_from_rows(cards: &[Card]) -> Option<String> {
    if cards.is_empty() {
        return None;
    }
    let has_card_metadata = cards.iter().any(|card| {
        !card.set.trim().is_empty()
            || !card.set_code.trim().is_empty()
            || !card.cn.trim().is_empty()
            || !card.rarity.trim().is_empty()
    });
    if has_card_metadata {
        None
    } else {
        Some(GENERIC_CATEGORY.to_string())
    }
}

#[cfg(test)]
#[path = "category_tests.rs"]
mod tests;
