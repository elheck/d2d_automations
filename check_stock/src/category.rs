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

/// Cardmarket's numeric game ID for Magic, as it appears in `inventory-report-45.csv`.
const MAGIC_GAME_ID: &str = "45";

/// Maps Cardmarket's numeric game IDs to the names we store.
///
/// Only IDs we have actually seen in exports are listed; anything else falls
/// through to the raw suffix so an unknown game still gets its own scope rather
/// than being silently merged into Magic.
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
/// Numeric suffixes are Cardmarket game IDs and are mapped to names; textual
/// suffixes (`Generic`) are used as-is. Returns `None` for filenames that carry
/// no usable suffix, leaving the decision to content inspection.
fn category_from_filename(path: &str) -> Option<String> {
    let stem = Path::new(path).file_stem()?.to_str()?;
    let suffix = strip_report_prefix(stem)?.trim();
    if suffix.is_empty() {
        return None;
    }

    if let Some((_, name)) = GAME_IDS
        .iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(suffix))
    {
        return Some((*name).to_string());
    }

    // A purely numeric suffix we don't have a name for is still a distinct game;
    // scope it under a stable synthetic name rather than guessing at Magic.
    if suffix.bytes().all(|b| b.is_ascii_digit()) {
        return Some(format!("Game{suffix}"));
    }

    Some(normalize_name(suffix))
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
        Some("Generic".to_string())
    }
}

#[cfg(test)]
#[path = "category_tests.rs"]
mod tests;
