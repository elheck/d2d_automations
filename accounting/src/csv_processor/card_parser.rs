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
#[path = "card_parser_tests.rs"]
mod tests;
