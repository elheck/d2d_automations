//! Inventory search: which cards match a search text in the chosen fields
//! (the Search screen of the desktop app and the web version).

use crate::models::Card;

/// The fields searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchFields {
    pub name: bool,
    pub set: bool,
    pub condition: bool,
    pub language: bool,
    pub location: bool,
    pub rarity: bool,
    pub price: bool,
    pub comment: bool,
}

impl Default for SearchFields {
    /// Name and set, as the desktop app starts.
    fn default() -> Self {
        Self {
            name: true,
            set: true,
            condition: false,
            language: false,
            location: false,
            rarity: false,
            price: false,
            comment: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchOptions {
    pub fields: SearchFields,
    pub case_sensitive: bool,
    /// Also the German, Spanish, French and Italian names (with `name`).
    pub all_languages: bool,
}

/// The search text as it is compared (lower case unless case sensitive).
pub fn prepare_term(term: &str, options: &SearchOptions) -> String {
    if options.case_sensitive {
        term.to_string()
    } else {
        term.to_lowercase()
    }
}

/// Whether `card` contains the prepared `term` in one of the chosen fields.
/// An empty term matches every card.
pub fn card_matches(card: &Card, term: &str, options: &SearchOptions) -> bool {
    if term.is_empty() {
        return true;
    }
    let f = &options.fields;
    let check = |on: bool, value: &str| {
        on && if options.case_sensitive {
            value.contains(term)
        } else {
            value.to_lowercase().contains(term)
        }
    };
    check(f.name, &card.name)
        || check(f.set, &card.set)
        || check(f.condition, &card.condition)
        || check(f.language, &card.language)
        || check(f.rarity, &card.rarity)
        || check(f.price, &card.price)
        || check(f.comment, &card.comment)
        || card
            .location
            .as_deref()
            .is_some_and(|location| check(f.location, location))
        || (options.all_languages
            && f.name
            && (check(true, &card.name_de)
                || check(true, &card.name_es)
                || check(true, &card.name_fr)
                || check(true, &card.name_it)))
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
