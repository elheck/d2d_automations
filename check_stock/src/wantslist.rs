//! Wantslist / decklist parsing.
//!
//! Pure, file-I/O-free logic for turning a pasted or exported deck list into a
//! set of [`WantsEntry`] rows. Handles the common community export formats so a
//! buylist or deck request can be dropped in as-is:
//!
//! * **Plain** — `4 Lightning Bolt`
//! * **`x` quantity** — `4x Lightning Bolt`, `1X Sol Ring` (Archidekt, TCGplayer)
//! * **MTG Arena / MTGO** — `4 Lightning Bolt (2XM) 123`, with `Deck` / `Sideboard`
//!   / `Commander` section headers and an `About` / `Name …` preamble
//! * **Moxfield** — `1 Lightning Bolt (2XM) 123 *F*` (foil/etched markers)
//! * **Archidekt** — `1x Lightning Bolt (2XM) 123 [Removal] ^Have,#7bb662^`
//! * **MTGGoldfish** — `SB:`-prefixed sideboard rows
//!
//! Card **matching** is by exact (case-insensitive) name, so the parser's job is
//! to strip every set code, collector number, foil marker, category and tag and
//! leave just the card name. Cards that appear more than once (e.g. maindeck and
//! sideboard) are merged, summing their quantities.

use crate::models::WantsEntry;
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    /// Leading quantity, with an optional `SB:` sideboard prefix and optional
    /// `x`/`X` multiplier suffix, then the rest of the line.
    static ref QTY_RE: Regex = Regex::new(r"^(?:SB:\s*)?(\d+)\s*[xX]?\s+(.+)$").unwrap();
    /// Archidekt caret tag groups, e.g. `^Have,#7bb662^`.
    static ref TAG_RE: Regex = Regex::new(r"\s*\^[^^]*\^").unwrap();
    /// Bracket category groups, e.g. `[Removal]`, `[Maybeboard{noPrice}]`.
    static ref CAT_RE: Regex = Regex::new(r"\s*\[[^\]]*\]").unwrap();
    /// Moxfield finish markers, e.g. `*F*` (foil), `*E*` (etched).
    static ref FOIL_RE: Regex = Regex::new(r"\s*\*[A-Za-z]\*").unwrap();
    /// Trailing `(SET)` code (short, space-free) plus an optional collector
    /// number, anchored at end of line. The space-free constraint avoids eating
    /// a genuine parenthetical in a card name (e.g. `B.F.M. (Big Furry Monster)`).
    static ref SET_RE: Regex =
        Regex::new(r"\s*\([A-Za-z0-9]{1,6}\)(?:\s+[0-9A-Za-z\-\u{2605}#]+)?\s*$").unwrap();
}

/// Section headers and preamble labels that carry no card and should be skipped.
const HEADERS: &[&str] = &[
    "deck",
    "sideboard",
    "commander",
    "companion",
    "maybeboard",
    "considering",
    "tokens",
    "planes",
    "schemes",
    "stickers",
    "attractions",
    "about",
];

/// Outcome of parsing a single line.
#[derive(Debug, PartialEq)]
pub enum ParsedLine {
    /// A card row.
    Entry { quantity: i32, name: String },
    /// A blank line, comment, section header or preamble — intentionally ignored.
    Skip,
    /// A non-empty line that did not look like a card row.
    Unparseable,
}

/// Result of parsing a whole wantslist.
#[derive(Debug, Default, PartialEq)]
pub struct WantslistParse {
    /// Card entries, de-duplicated by name (case-insensitive) with quantities summed.
    pub entries: Vec<WantsEntry>,
    /// Non-empty lines that could not be parsed as a card row (for reporting).
    pub unparseable: Vec<String>,
}

/// Removes set/collector/foil/category/tag annotations, leaving the bare name.
fn clean_name(raw: &str) -> String {
    let s = TAG_RE.replace_all(raw, "");
    let s = CAT_RE.replace_all(&s, "");
    let s = FOIL_RE.replace_all(&s, "");
    let s = SET_RE.replace(&s, "");
    s.trim().to_string()
}

/// Parses one line into a [`ParsedLine`].
pub fn parse_line(line: &str) -> ParsedLine {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ParsedLine::Skip;
    }
    // Comments: MTGO `//`, generic `#`.
    if trimmed.starts_with("//") || trimmed.starts_with('#') {
        return ParsedLine::Skip;
    }

    let lower = trimmed.to_lowercase();
    if HEADERS.contains(&lower.as_str()) || lower.starts_with("name ") {
        return ParsedLine::Skip;
    }

    if let Some(caps) = QTY_RE.captures(trimmed) {
        let quantity: i32 = match caps[1].parse() {
            Ok(q) => q,
            Err(_) => return ParsedLine::Unparseable,
        };
        let name = clean_name(&caps[2]);
        if name.is_empty() {
            return ParsedLine::Unparseable;
        }
        return ParsedLine::Entry { quantity, name };
    }

    ParsedLine::Unparseable
}

/// Parses a full wantslist, merging duplicate card names.
///
/// Duplicates are merged case-insensitively but the first-seen spelling and
/// order are preserved, so the output is stable and human-readable.
pub fn parse_wantslist(content: &str) -> WantslistParse {
    let mut result = WantslistParse::default();
    // Maps a lower-cased name to its index in `result.entries` for O(1) merging.
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for line in content.lines() {
        match parse_line(line) {
            ParsedLine::Entry { quantity, name } => {
                let key = name.to_lowercase();
                if let Some(&i) = index.get(&key) {
                    result.entries[i].quantity += quantity;
                } else {
                    index.insert(key, result.entries.len());
                    result.entries.push(WantsEntry { quantity, name });
                }
            }
            ParsedLine::Skip => {}
            ParsedLine::Unparseable => result.unparseable.push(line.trim().to_string()),
        }
    }

    result
}

#[path = "wantslist_tests.rs"]
#[cfg(test)]
mod tests;
