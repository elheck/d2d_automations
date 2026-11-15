//! Text for CSV files that people open in a spreadsheet.
//!
//! Excel and LibreOffice run a cell that starts with `=`, `+`, `-` or `@` as a
//! formula (CSV injection): a card name such as `=HYPERLINK("http://…")` in a
//! seller's export would turn into a live formula in the offer CSV. Such text
//! gets a leading `'`, which makes the spreadsheet show it as plain text.
//!
//! Only for reports that people read. Files that are imported somewhere
//! again (the repricing CSV for Cardmarket) must keep their values exactly.

use std::borrow::Cow;

/// Characters that start a formula: ASCII and their full-width forms.
const FORMULA_START: [char; 8] = ['=', '+', '-', '@', '＝', '＋', '－', '＠'];

/// `text` as plain text for a spreadsheet: prefixed with `'` if its first
/// non-blank character could start a formula, or if it starts with a tab or
/// a line break (which some spreadsheets strip before looking).
pub fn spreadsheet_safe(text: &str) -> Cow<'_, str> {
    let starts_with_control = text.starts_with(['\t', '\r', '\n']);
    let first_visible = text.trim_start().chars().next();
    if starts_with_control || first_visible.is_some_and(|c| FORMULA_START.contains(&c)) {
        Cow::Owned(format!("'{text}"))
    } else {
        Cow::Borrowed(text)
    }
}

#[cfg(test)]
#[path = "spreadsheet_tests.rs"]
mod tests;
