use crate::models::{Card, Language};
use std::collections::HashMap;

pub fn get_card_name(card: &Card, language: Option<Language>) -> &str {
    match language {
        Some(Language::German) if !card.name_de.is_empty() => &card.name_de,
        Some(Language::Spanish) if !card.name_es.is_empty() => &card.name_es,
        Some(Language::French) if !card.name_fr.is_empty() => &card.name_fr,
        Some(Language::Italian) if !card.name_it.is_empty() => &card.name_it,
        _ => &card.name,
    }
}

#[derive(Clone)]
pub struct MatchedCard<'a> {
    pub card: &'a Card,
    pub quantity: i32,
    pub set_name: String,
}

pub fn find_matching_cards<'a>(
    card_name: &str,
    needed_quantity: i32,
    inventory: &'a [Card],
    preferred_language: Option<Language>,
    preferred_language_only: bool,
) -> Vec<MatchedCard<'a>> {
    let trimmed_card_name = card_name.trim();
    let matching_cards: Vec<_> = inventory
        .iter()
        .filter(|card| {
            if preferred_language_only {
                if let Some(lang) = preferred_language {
                    get_card_name(card, Some(lang))
                        .trim()
                        .eq_ignore_ascii_case(trimmed_card_name)
                        && card.language.eq_ignore_ascii_case(lang.as_str())
                } else {
                    // If no preferred language is set, fallback to English
                    get_card_name(card, None)
                        .trim()
                        .eq_ignore_ascii_case(trimmed_card_name)
                        && card
                            .language
                            .eq_ignore_ascii_case(Language::English.as_str())
                }
            } else {
                // Match any language
                Language::all().iter().any(|lang| {
                    get_card_name(card, Some(*lang))
                        .trim()
                        .eq_ignore_ascii_case(trimmed_card_name)
                })
            }
        })
        .collect();

    if matching_cards.is_empty() {
        return Vec::new();
    }

    // Sort matched cards to prioritize preferred language
    let mut cards_by_set: HashMap<String, Vec<&Card>> = HashMap::new();
    for card in matching_cards {
        let set_key = format!("{} ({})", &card.set, &card.set_code);
        let cards = cards_by_set.entry(set_key).or_default();
        cards.push(card);
    }
    // Sort cards within each set: preferred language first, then by price, name, cardmarket_id
    for cards in cards_by_set.values_mut() {
        cards.sort_by(|a, b| {
            let lang_pref = |c: &Card| {
                if let Some(lang) = preferred_language {
                    Language::parse(&c.language)
                        .map(|card_lang| card_lang == lang)
                        .unwrap_or(false)
                } else {
                    false
                }
            };
            lang_pref(b)
                .cmp(&lang_pref(a)) // true first
                .then_with(|| {
                    let pa = a.price.parse::<f64>().unwrap_or(f64::MAX);
                    let pb = b.price.parse::<f64>().unwrap_or(f64::MAX);
                    pa.partial_cmp(&pb).unwrap()
                })
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.cardmarket_id.cmp(&b.cardmarket_id))
        });
    }

    let mut remaining_needed = needed_quantity;
    let mut result = Vec::new();

    // Sort sets by price, then by set name for determinism
    let mut sets: Vec<_> = cards_by_set.iter().collect();
    sets.sort_by(|a, b| {
        let price_a = a.1[0].price.parse::<f64>().unwrap_or(f64::MAX);
        let price_b = b.1[0].price.parse::<f64>().unwrap_or(f64::MAX);
        price_a
            .partial_cmp(&price_b)
            .unwrap()
            .then_with(|| b.0.cmp(a.0))
    });

    // Add cards from each set until we have enough
    for (set_name, cards) in sets {
        if remaining_needed <= 0 {
            break;
        }

        for card in cards {
            if remaining_needed <= 0 {
                break;
            }
            if let Ok(quantity) = card.quantity.parse::<i32>() {
                if quantity > 0 {
                    let effective_quantity = if card.is_playset_card() {
                        quantity * 4
                    } else {
                        quantity
                    };
                    let copies = remaining_needed.min(effective_quantity);
                    result.push(MatchedCard {
                        card,
                        quantity: copies,
                        set_name: set_name.clone(),
                    });
                    remaining_needed -= copies;
                }
            }
        }
    }

    result
}

pub fn parse_location_code(loc: &str) -> Vec<i32> {
    let main_part = loc.split("-L0").next().unwrap_or(loc);

    main_part
        .split('-')
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                match part.chars().next().unwrap_or('A') {
                    'A' => 1,
                    'B' => 2,
                    'C' => 3,
                    'D' => 4,
                    _ => 0,
                }
            } else {
                part.parse::<i32>().unwrap_or(0)
            }
        })
        .collect()
}
