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
mod tests {
    use super::*;

    mod parse_order_items_tests {
        use super::*;

        #[test]
        fn parses_single_item() {
            let items = parse_order_items("1x Card Name - 1,87 EUR", "12345", "Card Name").unwrap();

            assert_eq!(items.len(), 1);
            assert_eq!(items[0].product_id, "12345");
            assert_eq!(items[0].localized_product_name, "Card Name");
            assert_eq!(items[0].quantity, 1);
            assert!((items[0].price - 1.87).abs() < 0.001);
        }

        #[test]
        fn parses_multiple_items() {
            let items = parse_order_items(
                "1x Card One - 1,50 EUR | 2x Card Two - 3,00 EUR",
                "111 | 222",
                "Card One | Card Two",
            )
            .unwrap();

            assert_eq!(items.len(), 2);

            assert_eq!(items[0].product_id, "111");
            assert_eq!(items[0].localized_product_name, "Card One");
            assert_eq!(items[0].quantity, 1);
            assert!((items[0].price - 1.50).abs() < 0.001);

            assert_eq!(items[1].product_id, "222");
            assert_eq!(items[1].localized_product_name, "Card Two");
            assert_eq!(items[1].quantity, 2);
            assert!((items[1].price - 3.00).abs() < 0.001);
        }

        #[test]
        fn handles_mismatched_counts_as_single() {
            // When counts don't match, treat as single item
            let items = parse_order_items(
                "1x Card One - 1,50 EUR | 2x Card Two - 3,00 EUR",
                "111", // Only one ID
                "Card One | Card Two",
            )
            .unwrap();

            assert_eq!(items.len(), 1);
        }
    }

    mod parse_order_line_tests {
        use super::*;

        #[test]
        fn parses_valid_order_line() {
            let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

            let order = parse_order_line(line).unwrap();

            assert_eq!(order.order_id, "1234567");
            assert_eq!(order.username, "user123");
            assert_eq!(order.name, "John Doe");
            assert_eq!(order.street, "Main Street 1");
            assert_eq!(order.zip, "10557");
            assert_eq!(order.city, "Berlin");
            assert_eq!(order.country, "Germany");
            assert_eq!(order.date_of_purchase, "2025-01-15");
            assert_eq!(order.article_count, 1);
            assert_eq!(order.merchandise_value, "5,00");
            assert_eq!(order.shipment_costs, "1,50");
            assert_eq!(order.total_value, "6,50");
            assert_eq!(order.currency, "EUR");
            assert_eq!(order.product_id, "98765");
            assert_eq!(order.localized_product_name, "Card Name");
        }

        #[test]
        fn parses_order_with_optional_fields_empty() {
            let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

            let order = parse_order_line(line).unwrap();

            assert!(order.is_professional.is_none());
            assert!(order.vat_number.is_none());
        }

        #[test]
        fn parses_order_with_professional_flag() {
            let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;yes;DE123456789;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

            let order = parse_order_line(line).unwrap();

            assert_eq!(order.is_professional, Some("yes".to_string()));
            assert_eq!(order.vat_number, Some("DE123456789".to_string()));
        }

        #[test]
        fn fails_with_insufficient_columns() {
            let line = "1234567;user123;John Doe";

            let result = parse_order_line(line);
            assert!(result.is_err());
        }

        #[test]
        fn fails_with_invalid_article_count() {
            let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;not_a_number;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

            let result = parse_order_line(line);
            assert!(result.is_err());
        }
    }

    mod parse_csv_with_headers_tests {
        use super::*;

        #[test]
        fn parses_csv_with_headers() {
            let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName\n\
                          1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

            let orders = parse_csv_with_headers(content).unwrap();

            assert_eq!(orders.len(), 1);
            assert_eq!(orders[0].order_id, "1234567");
            assert_eq!(orders[0].name, "John Doe");
        }

        #[test]
        fn parses_multiple_orders() {
            let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName\n\
                          1234567;user1;John Doe;Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card One\n\
                          1234568;user2;Jane Doe;Street 2;20095 Hamburg;Germany;;;2025-01-16;2;10,00;1,50;11,50;0,20;EUR;2x Card - 5,00 EUR;98766;Card Two";

            let orders = parse_csv_with_headers(content).unwrap();

            assert_eq!(orders.len(), 2);
            assert_eq!(orders[0].name, "John Doe");
            assert_eq!(orders[1].name, "Jane Doe");
        }

        #[test]
        fn returns_empty_for_empty_content() {
            // Empty content has no header, so parse_csv_with_headers won't be called
            // But if it is, it should return empty
            let orders = parse_csv_with_headers("").unwrap();
            assert!(orders.is_empty());
        }

        #[test]
        fn returns_empty_for_header_only() {
            let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName";

            let orders = parse_csv_with_headers(content).unwrap();
            assert!(orders.is_empty());
        }

        #[test]
        fn skips_empty_lines() {
            let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName\n\
                          \n\
                          1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name\n\
                          ";

            let orders = parse_csv_with_headers(content).unwrap();
            assert_eq!(orders.len(), 1);
        }
    }
}
