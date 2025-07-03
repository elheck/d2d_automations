use std::collections::HashMap;
use crate::models::Card;

pub fn get_card_name<'a>(card: &'a Card, language: Option<&str>) -> &'a str {
    match language {
        Some("de") if !card.name_de.is_empty() => &card.name_de,
        Some("es") if !card.name_es.is_empty() => &card.name_es,
        Some("fr") if !card.name_fr.is_empty() => &card.name_fr,
        Some("it") if !card.name_it.is_empty() => &card.name_it,
        _ => &card.name
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
    preferred_language: Option<&str>
) -> Vec<MatchedCard<'a>> {
    // Find cards matching the name in any language, prioritizing preferred language
    let trimmed_card_name = card_name.trim();
    let matching_cards: Vec<_> = inventory.iter()
        .filter(|card| {
            // Check preferred language first
            if let Some(lang) = preferred_language {
                let localized_name = get_card_name(card, Some(lang)).trim();
                if localized_name.eq_ignore_ascii_case(trimmed_card_name) {
                    // If we find a match in preferred language, prioritize it
                    return true;
                }
            }

            // If no match in preferred language, check all languages
            let english_name = get_card_name(card, None).trim();
            english_name.eq_ignore_ascii_case(trimmed_card_name) ||
            get_card_name(card, Some("de")).trim().eq_ignore_ascii_case(trimmed_card_name) ||
            get_card_name(card, Some("es")).trim().eq_ignore_ascii_case(trimmed_card_name) ||
            get_card_name(card, Some("fr")).trim().eq_ignore_ascii_case(trimmed_card_name) ||
            get_card_name(card, Some("it")).trim().eq_ignore_ascii_case(trimmed_card_name)
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
        
        // Insert prioritizing preferred language
        if let Some(lang) = preferred_language {
            if card.language.eq_ignore_ascii_case(lang) || 
               (lang == "de" && card.language == "German") ||
               (lang == "fr" && card.language == "French") ||
               (lang == "es" && card.language == "Spanish") ||
               (lang == "it" && card.language == "Italian") {
                // Insert at the beginning for preferred language
                cards.insert(0, card);
            } else {
                // Append other languages
                cards.push(card);
            }
        } else {
            cards.push(card);
        }
    }

    let mut remaining_needed = needed_quantity;
    let mut result = Vec::new();

    // Sort sets by price
    let mut sets: Vec<_> = cards_by_set.iter().collect();
    sets.sort_by(|a, b| {
        let price_a = a.1[0].price.parse::<f64>().unwrap_or(f64::MAX);
        let price_b = b.1[0].price.parse::<f64>().unwrap_or(f64::MAX);
        price_a.partial_cmp(&price_b).unwrap()
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
                    let is_playset = card.is_playset.as_deref().map(|s| s == "1" || s.eq_ignore_ascii_case("true")).unwrap_or(false);
                    let effective_quantity = if is_playset {
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
    
    main_part.split('-')
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