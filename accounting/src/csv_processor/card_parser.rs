//! Card/inventory CSV parsing for card data formats.
//!
//! Parses card data that may come in tabular format (tab-separated or space-separated)
//! and converts it to OrderRecord format for processing.

use anyhow::Result;
use chrono::Utc;
use log::{debug, warn};

use crate::models::{CardRecord, OrderItem, OrderRecord};

use super::field_parsers::parse_price;

/// Parses a single line of card data.
///
/// Supports tab-separated or space-separated formats with the following structure:
/// Set info - Price - Product ID - Card Name
///
/// # Arguments
/// * `line` - A single line of card data
///
/// # Returns
/// A parsed CardRecord, or an error if the format is unrecognized.
pub fn parse_card_line(line: &str) -> Result<CardRecord> {
    debug!("Attempting to parse as card data: {line}");

    // Split by tab or multiple spaces
    let parts: Vec<String> = if line.contains('\t') {
        line.split('\t').map(|s| s.to_string()).collect()
    } else {
        // Split by multiple spaces (2 or more)
        let joined = line.split_whitespace().collect::<Vec<_>>().join(" ");
        joined
            .split("  ")
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .collect()
    };

    debug!("Split into {} parts: {:?}", parts.len(), parts);

    if parts.len() < 3 {
        return Err(anyhow::anyhow!("Not enough parts for card data"));
    }

    // Try to identify card format: Set info - Price - Product ID - Card Name
    let mut set_info = String::new();
    let mut price = String::new();
    let mut product_id = String::new();
    let mut card_name = String::new();

    // Look for price pattern (number,number EUR)
    let mut price_idx = None;
    let mut product_id_idx = None;

    for (i, part) in parts.iter().enumerate() {
        if part.contains("EUR") || part.contains("$") {
            price = part.trim().to_string();
            price_idx = Some(i);
            break;
        }
    }

    // Product ID is usually numeric and comes after price
    if let Some(price_i) = price_idx {
        for (i, part) in parts.iter().enumerate().skip(price_i + 1) {
            if part.trim().chars().all(|c| c.is_numeric()) {
                product_id = part.trim().to_string();
                product_id_idx = Some(i);
                break;
            }
        }
    }

    // Card name is everything after product ID
    if let Some(prod_i) = product_id_idx {
        if prod_i + 1 < parts.len() {
            card_name = parts[prod_i + 1..].join(" ").trim().to_string();
        }
    }

    // Set info is everything before price
    if let Some(price_i) = price_idx {
        set_info = parts[..price_i].join(" ").trim().to_string();
    }

    if card_name.is_empty() || product_id.is_empty() {
        return Err(anyhow::anyhow!("Could not parse card name or product ID"));
    }

    // Parse set info for more details
    let (set_name, collector_number, rarity, condition, language) = parse_set_info(&set_info);
    let currency = if price.contains("EUR") {
        "EUR"
    } else if price.contains("$") {
        "USD"
    } else {
        "EUR"
    };

    Ok(CardRecord {
        product_id,
        card_name,
        set_name,
        collector_number,
        rarity,
        condition,
        language,
        price,
        currency: currency.to_string(),
    })
}

/// Parses set information from a string.
///
/// Expected format: "Set Name - Collector Number - Rarity - Condition - Language"
///
/// # Arguments
/// * `set_info` - The set information string
///
/// # Returns
/// A tuple of (set_name, collector_number, rarity, condition, language)
pub fn parse_set_info(set_info: &str) -> (String, String, String, String, String) {
    let parts: Vec<&str> = set_info.split(" - ").collect();

    let set_name = parts.first().unwrap_or(&"Unknown").trim().to_string();
    let collector_number = parts.get(1).unwrap_or(&"").trim().to_string();
    let rarity = parts.get(2).unwrap_or(&"").trim().to_string();
    let condition = parts.get(3).unwrap_or(&"NM").trim().to_string();
    let language = parts.get(4).unwrap_or(&"English").trim().to_string();

    (set_name, collector_number, rarity, condition, language)
}

/// Converts a CardRecord to an OrderRecord.
///
/// This allows card inventory data to be processed through the same
/// invoice generation pipeline as regular orders.
///
/// # Arguments
/// * `card` - The card record to convert
///
/// # Returns
/// An OrderRecord with default values for order-specific fields.
pub fn card_to_order(card: CardRecord) -> OrderRecord {
    debug!("Converting card to order: {}", card.card_name);

    let parsed_price = parse_price(&card.price).unwrap_or_else(|e| {
        warn!("Failed to parse price '{}': {}, using 0.0", card.price, e);
        0.0
    });

    let item = OrderItem {
        description: format!(
            "{} - {} - {}",
            card.card_name, card.set_name, card.condition
        ),
        product_id: card.product_id.clone(),
        localized_product_name: card.card_name.clone(),
        price: parsed_price,
        quantity: 1, // Default to 1 for card-based orders
    };

    OrderRecord {
        order_id: card.product_id.clone(),
        username: "Card Inventory".to_string(),
        name: "Card Customer".to_string(),
        street: String::new(),
        zip: String::new(),
        city: String::new(),
        country: "DE".to_string(),
        is_professional: None,
        vat_number: None,
        date_of_purchase: Utc::now().format("%Y-%m-%d").to_string(),
        article_count: 1,
        merchandise_value: card.price.clone(),
        shipment_costs: "0.00".to_string(),
        total_value: card.price.clone(),
        commission: "0.00".to_string(),
        currency: card.currency,
        description: format!(
            "{} - {} - {}",
            card.card_name, card.set_name, card.condition
        ),
        product_id: card.product_id,
        localized_product_name: card.card_name,
        items: vec![item],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod parse_set_info_tests {
        use super::*;

        #[test]
        fn parses_complete_set_info() {
            let (set_name, collector_number, rarity, condition, language) =
                parse_set_info("Modern Horizons 2 - 142 - Rare - NM - English");

            assert_eq!(set_name, "Modern Horizons 2");
            assert_eq!(collector_number, "142");
            assert_eq!(rarity, "Rare");
            assert_eq!(condition, "NM");
            assert_eq!(language, "English");
        }

        #[test]
        fn handles_partial_set_info() {
            let (set_name, collector_number, rarity, condition, language) =
                parse_set_info("Modern Horizons 2 - 142");

            assert_eq!(set_name, "Modern Horizons 2");
            assert_eq!(collector_number, "142");
            assert_eq!(rarity, "");
            assert_eq!(condition, "NM"); // Default
            assert_eq!(language, "English"); // Default
        }

        #[test]
        fn handles_set_name_only() {
            let (set_name, collector_number, rarity, condition, language) =
                parse_set_info("Modern Horizons 2");

            assert_eq!(set_name, "Modern Horizons 2");
            assert_eq!(collector_number, "");
            assert_eq!(rarity, "");
            assert_eq!(condition, "NM");
            assert_eq!(language, "English");
        }

        #[test]
        fn handles_empty_string() {
            let (set_name, collector_number, rarity, condition, language) = parse_set_info("");

            assert_eq!(set_name, "");
            assert_eq!(collector_number, "");
            assert_eq!(rarity, "");
            assert_eq!(condition, "NM");
            assert_eq!(language, "English");
        }
    }

    mod parse_card_line_tests {
        use super::*;

        #[test]
        fn parses_tab_separated_card_line() {
            let line = "Modern Horizons 2 - 142 - Rare\t5,00 EUR\t12345\tCard Name";

            let card = parse_card_line(line).unwrap();

            assert_eq!(card.product_id, "12345");
            assert_eq!(card.card_name, "Card Name");
            assert_eq!(card.price, "5,00 EUR");
            assert_eq!(card.currency, "EUR");
            assert_eq!(card.set_name, "Modern Horizons 2");
        }

        #[test]
        fn parses_usd_currency() {
            let line = "Modern Horizons 2 - 142 - Rare\t$5.00\t12345\tCard Name";

            let card = parse_card_line(line).unwrap();

            assert_eq!(card.currency, "USD");
        }

        #[test]
        fn fails_with_insufficient_parts() {
            let line = "Card Name\t5,00 EUR";

            let result = parse_card_line(line);
            assert!(result.is_err());
        }

        #[test]
        fn fails_without_price_marker() {
            let line = "Card Name\t12345\tSome Info";

            let result = parse_card_line(line);
            // No EUR or $ marker means we can't find the price
            assert!(result.is_err());
        }
    }

    mod card_to_order_tests {
        use super::*;

        #[test]
        fn converts_card_to_order() {
            let card = CardRecord {
                product_id: "12345".to_string(),
                card_name: "Test Card".to_string(),
                set_name: "Test Set".to_string(),
                collector_number: "001".to_string(),
                rarity: "Rare".to_string(),
                condition: "NM".to_string(),
                language: "English".to_string(),
                price: "5,00".to_string(),
                currency: "EUR".to_string(),
            };

            let order = card_to_order(card);

            assert_eq!(order.order_id, "12345");
            assert_eq!(order.name, "Card Customer");
            assert_eq!(order.username, "Card Inventory");
            assert_eq!(order.country, "DE");
            assert_eq!(order.article_count, 1);
            assert_eq!(order.currency, "EUR");
            assert_eq!(order.items.len(), 1);
            assert_eq!(order.items[0].product_id, "12345");
            assert!((order.items[0].price - 5.0).abs() < 0.001);
        }

        #[test]
        fn handles_unparseable_price() {
            let card = CardRecord {
                product_id: "12345".to_string(),
                card_name: "Test Card".to_string(),
                set_name: "Test Set".to_string(),
                collector_number: "001".to_string(),
                rarity: "Rare".to_string(),
                condition: "NM".to_string(),
                language: "English".to_string(),
                price: "invalid price".to_string(),
                currency: "EUR".to_string(),
            };

            let order = card_to_order(card);

            // Should default to 0.0 when price can't be parsed
            assert!((order.items[0].price - 0.0).abs() < 0.001);
        }

        #[test]
        fn creates_proper_description() {
            let card = CardRecord {
                product_id: "12345".to_string(),
                card_name: "Test Card".to_string(),
                set_name: "Test Set".to_string(),
                collector_number: "001".to_string(),
                rarity: "Rare".to_string(),
                condition: "NM".to_string(),
                language: "English".to_string(),
                price: "5,00".to_string(),
                currency: "EUR".to_string(),
            };

            let order = card_to_order(card);

            assert!(order.description.contains("Test Card"));
            assert!(order.description.contains("Test Set"));
            assert!(order.description.contains("NM"));
        }
    }
}
