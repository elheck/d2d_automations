//! The quick card entry of the singles listing: a set code followed by a
//! three-digit collector number (`hou120`, `mh2130`, `2xm050`), or only the
//! three digits when a default set is given.

/// A printing to look up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardRef {
    /// Lower-case set code, e.g. `hou`.
    pub set_code: String,
    /// Collector number without leading zeros, e.g. `5` for `005`.
    pub collector_number: String,
}

/// `005` → `5`; `000` → `0` (Scryfall wants numbers without leading zeros).
fn strip_leading_zeros(digits: &str) -> String {
    let stripped = digits.trim_start_matches('0');
    if stripped.is_empty() {
        "0".to_string()
    } else {
        stripped.to_string()
    }
}

/// Parses `hou120`: the last three characters are the collector number (all
/// digits), everything before is the set code. `None` if that does not fit.
pub fn parse_card_input(input: &str) -> Option<CardRef> {
    let input = input.trim().to_lowercase();
    // At least one character of set code plus three digits.
    if input.len() < 4 || !input.is_ascii() {
        return None;
    }
    let (set_code, number) = input.split_at(input.len() - 3);
    if !number.chars().all(|c| c.is_ascii_digit()) || set_code.is_empty() {
        return None;
    }
    Some(CardRef {
        set_code: set_code.to_string(),
        collector_number: strip_leading_zeros(number),
    })
}

/// Like [`parse_card_input`], but exactly three digits mean "this number in
/// `default_set`" when a default set is given.
pub fn resolve_card_input(input: &str, default_set: &str) -> Option<CardRef> {
    let input = input.trim();
    let default_set = default_set.trim();
    if input.is_empty() {
        return None;
    }
    if input.len() == 3 && input.chars().all(|c| c.is_ascii_digit()) && !default_set.is_empty() {
        return Some(CardRef {
            set_code: default_set.to_lowercase(),
            collector_number: strip_leading_zeros(input),
        });
    }
    parse_card_input(input)
}

#[cfg(test)]
#[path = "card_input_tests.rs"]
mod tests;
