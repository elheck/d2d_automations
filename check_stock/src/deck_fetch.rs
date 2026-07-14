//! Fetch decklists directly from Moxfield and Archidekt share links.
//!
//! A wantslist field may contain a deck URL instead of a file path, e.g.
//! `https://moxfield.com/decks/aeorrmvviUiIVihw0bcL3A` or
//! `https://archidekt.com/decks/123456/my_deck`. [`parse_deck_url`] recognises
//! those links and [`fetch_deck`] downloads the deck via each site's public JSON
//! API and returns [`WantsEntry`] rows (duplicate names merged).
//!
//! The JSON-shaping logic is split into pure functions ([`parse_moxfield_json`],
//! [`parse_archidekt_json`]) so it can be tested without any network access.

use crate::models::WantsEntry;
use serde_json::Value;
use std::time::Duration;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// A recognised deck-hosting source and its deck identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeckSource {
    Moxfield(String),
    Archidekt(String),
}

/// Extracts the case-sensitive deck id that follows `marker` in `original`.
///
/// `lower` is the lowercased `original`, used to locate `marker`
/// case-insensitively; the id is then sliced from `original` so mixed-case
/// Moxfield ids survive. The id ends at the first `/`, `?` or `#`. The byte
/// offset is shared because everything up to the id in these URLs is ASCII.
fn extract_id(original: &str, lower: &str, marker: &str) -> Option<String> {
    let start = lower.find(marker)? + marker.len();
    let rest = &original[start..];
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let seg = rest[..end].trim();
    (!seg.is_empty()).then(|| seg.to_string())
}

/// Recognises a Moxfield or Archidekt deck URL and extracts its deck id.
///
/// Accepts `http`/`https` and optional `www.`. Returns `None` for anything else
/// (so plain file paths fall through to file loading).
pub fn parse_deck_url(input: &str) -> Option<DeckSource> {
    let s = input.trim();
    let lower = s.to_lowercase();
    // Only treat it as a URL if it actually looks like one.
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return None;
    }
    if let Some(id) = extract_id(s, &lower, "moxfield.com/decks/") {
        return Some(DeckSource::Moxfield(id));
    }
    if let Some(id) = extract_id(s, &lower, "archidekt.com/decks/") {
        return Some(DeckSource::Archidekt(id));
    }
    None
}

/// Downloads and parses a deck from its source. Blocking network call.
pub fn fetch_deck(source: &DeckSource) -> Result<Vec<WantsEntry>, String> {
    let (url, host) = match source {
        DeckSource::Moxfield(id) => (
            format!("https://api.moxfield.com/v2/decks/all/{id}"),
            "Moxfield",
        ),
        DeckSource::Archidekt(id) => (
            format!("https://archidekt.com/api/decks/{id}/"),
            "Archidekt",
        ),
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(&url)
        .header("User-Agent", mtg_common::USER_AGENT)
        .header("Accept", "application/json")
        .send()
        .map_err(|e| format!("Could not reach {host}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("{host} returned HTTP {}", resp.status()));
    }

    let body = resp.text().map_err(|e| e.to_string())?;
    match source {
        DeckSource::Moxfield(_) => parse_moxfield_json(&body),
        DeckSource::Archidekt(_) => parse_archidekt_json(&body),
    }
}

/// Merges entries by case-insensitive name, summing quantities and preserving
/// first-seen spelling and order.
fn merge(entries: Vec<WantsEntry>) -> Vec<WantsEntry> {
    let mut out: Vec<WantsEntry> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for e in entries {
        let key = e.name.to_lowercase();
        if let Some(&i) = index.get(&key) {
            out[i].quantity += e.quantity;
        } else {
            index.insert(key, out.len());
            out.push(e);
        }
    }
    out
}

/// Parses a Moxfield deck JSON body (v2 top-level boards or v3 `boards.*.cards`).
pub fn parse_moxfield_json(body: &str) -> Result<Vec<WantsEntry>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("Invalid Moxfield JSON: {e}"))?;

    // Boards that represent cards a player actually needs. `maybeboard` is
    // intentionally excluded.
    const BOARDS: [&str; 4] = ["mainboard", "sideboard", "commanders", "companions"];

    let mut entries = Vec::new();
    for board in BOARDS {
        // v3 nests card maps under boards.<name>.cards; v2 exposes them at the top.
        let board_val = v
            .get("boards")
            .and_then(|b| b.get(board))
            .or_else(|| v.get(board));
        let Some(board_val) = board_val else { continue };
        let cards = board_val.get("cards").unwrap_or(board_val);
        let Some(obj) = cards.as_object() else {
            continue;
        };
        for (key, item) in obj {
            let quantity = item.get("quantity").and_then(Value::as_i64).unwrap_or(1) as i32;
            let name = item
                .get("card")
                .and_then(|c| c.get("name"))
                .and_then(Value::as_str)
                .unwrap_or(key)
                .trim()
                .to_string();
            if quantity > 0 && !name.is_empty() {
                entries.push(WantsEntry { quantity, name });
            }
        }
    }

    if entries.is_empty() {
        return Err("No cards found in the Moxfield deck".to_string());
    }
    Ok(merge(entries))
}

/// Parses an Archidekt deck JSON body (`cards[]` with quantities and categories).
pub fn parse_archidekt_json(body: &str) -> Result<Vec<WantsEntry>, String> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| format!("Invalid Archidekt JSON: {e}"))?;

    let cards = v
        .get("cards")
        .and_then(Value::as_array)
        .ok_or("Archidekt JSON missing 'cards' array")?;

    let mut entries = Vec::new();
    for c in cards {
        // Skip cards filed only under the Maybeboard.
        let in_maybeboard = c
            .get("categories")
            .and_then(Value::as_array)
            .map(|cats| {
                cats.iter()
                    .filter_map(Value::as_str)
                    .any(|cat| cat.eq_ignore_ascii_case("maybeboard"))
            })
            .unwrap_or(false);
        if in_maybeboard {
            continue;
        }

        let quantity = c.get("quantity").and_then(Value::as_i64).unwrap_or(1) as i32;
        // Oracle name is the canonical printed name; fall back to the printing name.
        let card = c.get("card");
        let name = card
            .and_then(|card| card.get("oracleCard"))
            .and_then(|o| o.get("name"))
            .or_else(|| card.and_then(|card| card.get("name")))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        if quantity > 0 && !name.is_empty() {
            entries.push(WantsEntry { quantity, name });
        }
    }

    if entries.is_empty() {
        return Err("No cards found in the Archidekt deck".to_string());
    }
    Ok(merge(entries))
}

#[path = "deck_fetch_tests.rs"]
#[cfg(test)]
mod tests;
