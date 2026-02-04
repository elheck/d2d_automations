//! Order CSV parsing for Cardmarket export format.
//!
//! Parses CSV files exported from Cardmarket containing order data.

use anyhow::{Context, Result};
use log::{debug, info, warn};

use crate::models::{OrderItem, OrderRecord};

use super::field_parsers::{
    extract_price_from_description, extract_quantity_from_description, parse_city_field,
};

/// Parses CSV content with headers (Cardmarket export format).
///
/// Expects a semicolon-separated CSV with the following columns:
/// OrderID, Username, Name, Street, City, Country, IsProfessional, VATNumber,
/// DateOfPurchase, ArticleCount, MerchandiseValue, ShipmentCosts, TotalValue,
/// Commission, Currency, Description, ProductID, LocalizedProductName
///
/// # Arguments
/// * `content` - The raw CSV content as a string
///
/// # Returns
/// A vector of parsed OrderRecord, or an error if parsing fails.
pub fn parse_csv_with_headers(content: &str) -> Result<Vec<OrderRecord>> {
    debug!("Parsing CSV with headers");
    let mut orders = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    if lines.len() < 2 {
        warn!("CSV file has no data rows");
        return Ok(orders);
    }

    // Skip header row
    let data_lines = &lines[1..];
    info!(
        "Processing {} data lines (excluding header)",
        data_lines.len()
    );

    for (line_num, line) in data_lines.iter().enumerate() {
        if line.trim().is_empty() {
            debug!("Skipping empty line {}", line_num + 2);
            continue;
        }

        match parse_order_line(line) {
            Ok(order) => {
                debug!("Successfully parsed order: {:?}", order.order_id);
                orders.push(order);
            }
            Err(e) => {
                warn!("Failed to parse line {}: {}", line_num + 2, e);
                return Err(e);
            }
        }
    }

    info!("Successfully parsed {} orders from CSV", orders.len());
    Ok(orders)
}

/// Parses a single order line from the CSV.
///
/// # Arguments
/// * `line` - A semicolon-separated line from the CSV
///
/// # Returns
/// A parsed OrderRecord, or an error if the line is malformed.
pub fn parse_order_line(line: &str) -> Result<OrderRecord> {
    debug!("Parsing CSV line: {line}");
    let parts: Vec<&str> = line.split(';').collect();

    if parts.len() < 18 {
        let error_msg = format!(
            "Invalid CSV format. Expected at least 18 columns, got {}",
            parts.len()
        );
        warn!("{error_msg}");
        return Err(anyhow::anyhow!(error_msg));
    }

    // Parse the city field which contains both postal code and city name
    let city_field = parts[4].trim();
    let (zip, city) = parse_city_field(city_field)?;

    // Parse individual items if multiple items are present
    let items = parse_order_items(parts[15].trim(), parts[16].trim(), parts[17].trim())?;

    let order_record = OrderRecord {
        order_id: parts[0].trim().to_string(),
        username: parts[1].trim().to_string(),
        name: parts[2].trim().to_string(),
        street: parts[3].trim().to_string(),
        zip,
        city,
        country: parts[5].trim().to_string(),
        is_professional: if parts[6].trim().is_empty() {
            None
        } else {
            Some(parts[6].trim().to_string())
        },
        vat_number: if parts[7].trim().is_empty() {
            None
        } else {
            Some(parts[7].trim().to_string())
        },
        date_of_purchase: parts[8].trim().to_string(),
        article_count: parts[9]
            .trim()
            .parse::<u32>()
            .context("Failed to parse article count as number")?,
        merchandise_value: parts[10].trim().to_string(),
        shipment_costs: parts[11].trim().to_string(),
        total_value: parts[12].trim().to_string(),
        commission: parts[13].trim().to_string(),
        currency: parts[14].trim().to_string(),
        description: parts[15].trim().to_string(),
        product_id: parts[16].trim().to_string(),
        localized_product_name: parts[17].trim().to_string(),
        items,
    };

    debug!(
        "Parsed order record: {:?} with {} items",
        order_record.order_id,
        order_record.items.len()
    );
    Ok(order_record)
}

/// Parses order items from description, product IDs, and product names.
///
/// Handles both single-item and multi-item orders. Multi-item orders use
/// " | " as a delimiter between items.
///
/// # Note on delimiter handling
/// Card names can contain " | " (e.g., "Magic: The Gathering | Marvel's Spider-Man"),
/// so we use the Product ID count as authoritative (IDs are numeric and never contain pipes).
/// Descriptions are then split to match the expected item count.
///
/// # Arguments
/// * `description` - Item description(s), possibly " | "-separated
/// * `product_ids` - Product ID(s), possibly " | "-separated
/// * `product_names` - Product name(s), possibly " | "-separated
///
/// # Returns
/// A vector of parsed OrderItem.
pub fn parse_order_items(
    description: &str,
    product_ids: &str,
    product_names: &str,
) -> Result<Vec<OrderItem>> {
    let ids = product_ids.split(" | ").collect::<Vec<&str>>();
    let names = product_names.split(" | ").collect::<Vec<&str>>();

    // Use product ID count as authoritative - IDs are numeric and never contain " | "
    let expected_count = ids.len();

    // Check if we have multiple items (IDs and names must match)
    if expected_count > 1 && ids.len() == names.len() {
        info!("Parsing {} individual items from order", expected_count);

        // Split descriptions carefully - card names may contain " | "
        let descriptions = split_descriptions_by_count(description, expected_count);

        if descriptions.len() != expected_count {
            warn!(
                "Description count {} doesn't match expected {}, falling back to single item",
                descriptions.len(),
                expected_count
            );
            return parse_as_single_item(description, product_ids, product_names);
        }

        let mut items = Vec::new();
        for ((desc, id), name) in descriptions.iter().zip(ids.iter()).zip(names.iter()) {
            let price = extract_price_from_description(desc)?;
            let quantity = extract_quantity_from_description(desc);
            items.push(OrderItem {
                description: desc.trim().to_string(),
                product_id: id.trim().to_string(),
                localized_product_name: name.trim().to_string(),
                price,
                quantity,
            });
        }

        debug!("Successfully parsed {} items", items.len());
        Ok(items)
    } else {
        parse_as_single_item(description, product_ids, product_names)
    }
}

/// Splits description string into exactly `count` parts.
///
/// Uses " | " as delimiter but handles cases where card names contain " | "
/// by looking for the pattern that starts each item (e.g., "1x ", "2x ").
fn split_descriptions_by_count(description: &str, count: usize) -> Vec<String> {
    if count <= 1 {
        return vec![description.to_string()];
    }

    // First try simple split
    let simple_split: Vec<&str> = description.split(" | ").collect();
    if simple_split.len() == count {
        return simple_split.iter().map(|s| s.to_string()).collect();
    }

    // If simple split gives more parts than expected, we need to reconstruct
    // by finding items that start with quantity patterns (e.g., "1x ", "2x ")
    debug!(
        "Simple split gave {} parts, expected {}. Attempting smart split.",
        simple_split.len(),
        count
    );

    let mut result = Vec::new();
    let mut current_item = String::new();

    for (i, part) in simple_split.iter().enumerate() {
        let trimmed = part.trim();

        // Check if this part starts a new item (begins with quantity like "1x " or "2x ")
        let starts_new_item = trimmed
            .split_whitespace()
            .next()
            .map(|first_word| {
                first_word.ends_with('x')
                    && first_word[..first_word.len() - 1]
                        .chars()
                        .all(|c| c.is_ascii_digit())
            })
            .unwrap_or(false);

        if i == 0 {
            // First part always starts an item
            current_item = trimmed.to_string();
        } else if starts_new_item {
            // This starts a new item, save current and start new
            result.push(current_item);
            current_item = trimmed.to_string();
        } else {
            // This is a continuation of the previous item (embedded " | " in card name)
            current_item.push_str(" | ");
            current_item.push_str(trimmed);
        }
    }

    // Don't forget the last item
    if !current_item.is_empty() {
        result.push(current_item);
    }

    result
}

/// Parses the order as a single item (fallback case).
fn parse_as_single_item(
    description: &str,
    product_ids: &str,
    product_names: &str,
) -> Result<Vec<OrderItem>> {
    let price = extract_price_from_description(description).unwrap_or(0.0);
    let quantity = extract_quantity_from_description(description);
    let item = OrderItem {
        description: description.to_string(),
        product_id: product_ids.to_string(),
        localized_product_name: product_names.to_string(),
        price,
        quantity,
    };

    debug!("Single item order, price: {price:.2}, quantity: {quantity}");
    Ok(vec![item])
}

#[cfg(test)]
#[path = "order_parser_tests.rs"]
mod tests;
