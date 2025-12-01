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
    let descriptions = description.split(" | ").collect::<Vec<&str>>();
    let ids = product_ids.split(" | ").collect::<Vec<&str>>();
    let names = product_names.split(" | ").collect::<Vec<&str>>();

    // Check if we have multiple items
    if descriptions.len() == ids.len() && ids.len() == names.len() && descriptions.len() > 1 {
        info!("Parsing {} individual items from order", descriptions.len());
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
        // Single item - extract price from description if possible
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
}

#[cfg(test)]
#[path = "order_parser_tests.rs"]
mod tests;
