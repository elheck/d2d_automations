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
    // Find cards matching the name in preferred language
    let matching_cards: Vec<_> = inventory.iter()
        .filter(|card| {
            let name = get_card_name(card, preferred_language);
            name.eq_ignore_ascii_case(card_name)
        })
        .collect();

    if matching_cards.is_empty() {
        return Vec::new();
    }

    // Group cards by set
    let mut cards_by_set: HashMap<String, Vec<&Card>> = HashMap::new();
    for card in &matching_cards {
        let set_key = format!("{} ({})", &card.set, &card.set_code);
        cards_by_set.entry(set_key).or_default().push(card);
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

        let mut cards_vec = cards.clone();
        cards_vec.sort_by(|a, b| {
            let price_a = a.price.parse::<f64>().unwrap_or(f64::MAX);
            let price_b = b.price.parse::<f64>().unwrap_or(f64::MAX);
            price_a.partial_cmp(&price_b).unwrap()
        });

        for card in cards_vec {
            if remaining_needed <= 0 {
                break;
            }
            if let Ok(quantity) = card.quantity.parse::<i32>() {
                if quantity > 0 {
                    let effective_quantity = if card.is_playset == "1" || card.is_playset.to_lowercase() == "true" {
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