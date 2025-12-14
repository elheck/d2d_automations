use crate::models::{Card, Language};
use log::debug;
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
        debug!("No matches found for '{}'", trimmed_card_name);
        return Vec::new();
    }

    debug!(
        "Found {} potential matches for '{}'",
        matching_cards.len(),
        trimmed_card_name
    );

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

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a test card with default values
    fn create_test_card(name: &str, language: &str) -> Card {
        Card {
            cardmarket_id: "12345".to_string(),
            quantity: "4".to_string(),
            name: name.to_string(),
            set: "Alpha".to_string(),
            set_code: "LEA".to_string(),
            cn: "123".to_string(),
            condition: "NM".to_string(),
            language: language.to_string(),
            is_foil: "false".to_string(),
            is_playset: None,
            is_signed: "false".to_string(),
            price: "25.50".to_string(),
            comment: "".to_string(),
            location: Some("A-0-1-1".to_string()),
            name_de: "".to_string(),
            name_es: "".to_string(),
            name_fr: "".to_string(),
            name_it: "".to_string(),
            rarity: "common".to_string(),
            listed_at: "2024-01-01".to_string(),
        }
    }

    fn create_card_with_translations(
        name: &str,
        name_de: &str,
        name_es: &str,
        name_fr: &str,
        name_it: &str,
    ) -> Card {
        Card {
            cardmarket_id: "12345".to_string(),
            quantity: "4".to_string(),
            name: name.to_string(),
            set: "Alpha".to_string(),
            set_code: "LEA".to_string(),
            cn: "123".to_string(),
            condition: "NM".to_string(),
            language: "English".to_string(),
            is_foil: "false".to_string(),
            is_playset: None,
            is_signed: "false".to_string(),
            price: "25.50".to_string(),
            comment: "".to_string(),
            location: Some("A-0-1-1".to_string()),
            name_de: name_de.to_string(),
            name_es: name_es.to_string(),
            name_fr: name_fr.to_string(),
            name_it: name_it.to_string(),
            rarity: "common".to_string(),
            listed_at: "2024-01-01".to_string(),
        }
    }

    // ==================== get_card_name Tests ====================

    #[test]
    fn test_get_card_name_no_language_returns_english() {
        let card = create_card_with_translations(
            "Lightning Bolt",
            "Blitzschlag",
            "Rayo",
            "Éclair",
            "Fulmine",
        );
        assert_eq!(get_card_name(&card, None), "Lightning Bolt");
    }

    #[test]
    fn test_get_card_name_english_returns_english() {
        let card = create_card_with_translations(
            "Lightning Bolt",
            "Blitzschlag",
            "Rayo",
            "Éclair",
            "Fulmine",
        );
        assert_eq!(
            get_card_name(&card, Some(Language::English)),
            "Lightning Bolt"
        );
    }

    #[test]
    fn test_get_card_name_german_returns_german() {
        let card = create_card_with_translations(
            "Lightning Bolt",
            "Blitzschlag",
            "Rayo",
            "Éclair",
            "Fulmine",
        );
        assert_eq!(get_card_name(&card, Some(Language::German)), "Blitzschlag");
    }

    #[test]
    fn test_get_card_name_spanish_returns_spanish() {
        let card = create_card_with_translations(
            "Lightning Bolt",
            "Blitzschlag",
            "Rayo",
            "Éclair",
            "Fulmine",
        );
        assert_eq!(get_card_name(&card, Some(Language::Spanish)), "Rayo");
    }

    #[test]
    fn test_get_card_name_french_returns_french() {
        let card = create_card_with_translations(
            "Lightning Bolt",
            "Blitzschlag",
            "Rayo",
            "Éclair",
            "Fulmine",
        );
        assert_eq!(get_card_name(&card, Some(Language::French)), "Éclair");
    }

    #[test]
    fn test_get_card_name_italian_returns_italian() {
        let card = create_card_with_translations(
            "Lightning Bolt",
            "Blitzschlag",
            "Rayo",
            "Éclair",
            "Fulmine",
        );
        assert_eq!(get_card_name(&card, Some(Language::Italian)), "Fulmine");
    }

    #[test]
    fn test_get_card_name_german_empty_falls_back_to_english() {
        let card = create_card_with_translations("Lightning Bolt", "", "Rayo", "Éclair", "Fulmine");
        assert_eq!(
            get_card_name(&card, Some(Language::German)),
            "Lightning Bolt"
        );
    }

    #[test]
    fn test_get_card_name_all_translations_empty() {
        let card = create_card_with_translations("Lightning Bolt", "", "", "", "");
        assert_eq!(
            get_card_name(&card, Some(Language::German)),
            "Lightning Bolt"
        );
        assert_eq!(
            get_card_name(&card, Some(Language::Spanish)),
            "Lightning Bolt"
        );
        assert_eq!(
            get_card_name(&card, Some(Language::French)),
            "Lightning Bolt"
        );
        assert_eq!(
            get_card_name(&card, Some(Language::Italian)),
            "Lightning Bolt"
        );
    }

    // ==================== parse_location_code Tests ====================

    #[test]
    fn test_parse_location_code_simple() {
        let result = parse_location_code("A-0-1-4");
        assert_eq!(result, vec![1, 0, 1, 4]);
    }

    #[test]
    fn test_parse_location_code_with_l0_suffix() {
        let result = parse_location_code("A-0-1-4-L0-R");
        assert_eq!(result, vec![1, 0, 1, 4]);
    }

    #[test]
    fn test_parse_location_code_section_b() {
        let result = parse_location_code("B-1-2-3");
        assert_eq!(result, vec![2, 1, 2, 3]);
    }

    #[test]
    fn test_parse_location_code_section_c() {
        let result = parse_location_code("C-5-10-15");
        assert_eq!(result, vec![3, 5, 10, 15]);
    }

    #[test]
    fn test_parse_location_code_section_d() {
        let result = parse_location_code("D-0-0-0");
        assert_eq!(result, vec![4, 0, 0, 0]);
    }

    #[test]
    fn test_parse_location_code_unknown_section() {
        let result = parse_location_code("X-1-2-3");
        assert_eq!(result, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_parse_location_code_empty() {
        let result = parse_location_code("");
        // Empty string parses first char as 'A' default -> 1
        assert_eq!(result, vec![1]);
    }

    #[test]
    fn test_parse_location_code_invalid_numbers() {
        let result = parse_location_code("A-x-y-z");
        assert_eq!(result, vec![1, 0, 0, 0]);
    }

    // ==================== find_matching_cards Tests ====================

    #[test]
    fn test_find_matching_cards_exact_match() {
        let inventory = vec![create_test_card("Lightning Bolt", "English")];
        let matches = find_matching_cards("Lightning Bolt", 1, &inventory, None, false);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].quantity, 1);
    }

    #[test]
    fn test_find_matching_cards_case_insensitive() {
        let inventory = vec![create_test_card("Lightning Bolt", "English")];
        let matches = find_matching_cards("LIGHTNING BOLT", 1, &inventory, None, false);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_find_matching_cards_with_whitespace() {
        let inventory = vec![create_test_card("Lightning Bolt", "English")];
        let matches = find_matching_cards("  Lightning Bolt  ", 1, &inventory, None, false);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_find_matching_cards_no_match() {
        let inventory = vec![create_test_card("Lightning Bolt", "English")];
        let matches = find_matching_cards("Black Lotus", 1, &inventory, None, false);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_find_matching_cards_preferred_language_only() {
        let card_en = create_test_card("Lightning Bolt", "English");
        let mut card_de = create_test_card("Lightning Bolt", "German");
        card_de.name_de = "Blitzschlag".to_string();

        let inventory = vec![card_en, card_de];
        let matches = find_matching_cards(
            "Lightning Bolt",
            1,
            &inventory,
            Some(Language::English),
            true,
        );

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].card.language, "English");
    }

    #[test]
    fn test_find_matching_cards_quantity_limiting() {
        let mut card = create_test_card("Lightning Bolt", "English");
        card.quantity = "10".to_string();

        let inventory = vec![card];
        let matches = find_matching_cards("Lightning Bolt", 3, &inventory, None, false);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].quantity, 3); // Only takes what's needed
    }

    #[test]
    fn test_find_matching_cards_not_enough_quantity() {
        let mut card = create_test_card("Lightning Bolt", "English");
        card.quantity = "2".to_string();

        let inventory = vec![card];
        let matches = find_matching_cards("Lightning Bolt", 5, &inventory, None, false);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].quantity, 2); // Takes all available
    }
}
