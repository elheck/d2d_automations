use anyhow::{Context, Result};
use chrono::Utc;
use log::{debug, info, warn};
use std::path::Path;

use crate::models::{CardRecord, OrderRecord};

pub struct CsvProcessor;

impl CsvProcessor {
    pub fn new() -> Self {
        debug!("Creating new CSV processor");
        Self
    }

    pub async fn load_orders_from_csv<P: AsRef<Path>>(
        &self,
        file_path: P,
    ) -> Result<Vec<OrderRecord>> {
        let path = file_path.as_ref();
        info!("Loading orders from CSV file: {path:?}");

        let file_content = tokio::fs::read_to_string(path)
            .await
            .context("Failed to read CSV file")?;

        debug!("CSV file size: {} bytes", file_content.len());
        self.parse_csv_content(&file_content)
    }

    fn parse_csv_content(&self, content: &str) -> Result<Vec<OrderRecord>> {
        debug!("Starting CSV content parsing");
        let mut orders = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        if lines.is_empty() {
            warn!("CSV file is empty");
            return Ok(orders);
        }

        // Check if this is a proper CSV with headers
        let header_line = lines[0];
        debug!("Header line: {header_line}");

        // If it contains typical CSV headers, parse as CSV
        if header_line.contains("OrderID")
            || header_line.contains("Username")
            || header_line.contains("Name")
        {
            info!("Detected CSV format with headers");
            return self.parse_csv_with_headers(content);
        }

        // Otherwise try to parse as card data
        info!("Attempting to parse as card data format");
        for (line_num, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                debug!("Skipping empty line {}", line_num + 1);
                continue;
            }

            match self.parse_card_line(line) {
                Ok(card) => {
                    debug!("Successfully parsed card: {}", card.card_name);
                    orders.push(self.card_to_order(card));
                }
                Err(e) => {
                    warn!("Failed to parse line {} as card data: {}", line_num + 1, e);
                    return Err(e);
                }
            }
        }

        info!("Successfully parsed {} orders from card data", orders.len());
        Ok(orders)
    }

    fn parse_csv_with_headers(&self, content: &str) -> Result<Vec<OrderRecord>> {
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

            match self.parse_order_line(line) {
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

    fn parse_order_line(&self, line: &str) -> Result<OrderRecord> {
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
        let (zip, city) = self.parse_city_field(city_field)?;

        // Parse individual items if multiple items are present
        let items = self.parse_order_items(parts[15].trim(), parts[16].trim(), parts[17].trim())?;

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

    fn parse_order_items(
        &self,
        description: &str,
        product_ids: &str,
        product_names: &str,
    ) -> Result<Vec<crate::models::OrderItem>> {
        use crate::models::OrderItem;

        let descriptions = description.split(" | ").collect::<Vec<&str>>();
        let ids = product_ids.split(" | ").collect::<Vec<&str>>();
        let names = product_names.split(" | ").collect::<Vec<&str>>();

        // Check if we have multiple items
        if descriptions.len() == ids.len() && ids.len() == names.len() && descriptions.len() > 1 {
            info!("Parsing {} individual items from order", descriptions.len());
            let mut items = Vec::new();

            for ((desc, id), name) in descriptions.iter().zip(ids.iter()).zip(names.iter()) {
                let price = self.extract_price_from_description(desc)?;
                let quantity = self.extract_quantity_from_description(desc);
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
            let price = self
                .extract_price_from_description(description)
                .unwrap_or(0.0);
            let quantity = self.extract_quantity_from_description(description);
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

    fn extract_quantity_from_description(&self, description: &str) -> u32 {
        debug!("Extracting quantity from description: {description}");
        
        // Look for patterns like "2x", "10x", etc. at the beginning of the description
        if let Some(first_part) = description.split_whitespace().next() {
            if first_part.ends_with('x') {
                if let Ok(quantity) = first_part[..first_part.len()-1].parse::<u32>() {
                    debug!("Found quantity: {quantity}");
                    return quantity;
                }
            }
        }
        
        debug!("No quantity found, defaulting to 1");
        1 // Default to 1 if no quantity is found
    }

    fn extract_price_from_description(&self, description: &str) -> Result<f64> {
        debug!("Extracting price from description: {description}");

        // Look for pattern like "- 0,19 EUR" or "- 5,35 EUR"
        if let Some(price_match) = description.split(" - ").last() {
            if price_match.contains("EUR") {
                let price_str = price_match.replace("EUR", "").trim().replace(',', ".");
                if let Ok(price) = price_str.parse::<f64>() {
                    debug!("Extracted price: {price:.2}");
                    return Ok(price);
                }
            }
        }

        // Fallback: look for any number followed by EUR
        for part in description.split_whitespace() {
            if part.contains("EUR") {
                let price_str = part.replace("EUR", "").replace(',', ".");
                if let Ok(price) = price_str.parse::<f64>() {
                    debug!("Extracted price (fallback): {price:.2}");
                    return Ok(price);
                }
            }
        }

        warn!("Could not extract price from description: {description}");
        Err(anyhow::anyhow!("Could not extract price from description"))
    }

    fn parse_city_field(&self, city_field: &str) -> Result<(String, String)> {
        debug!("Parsing city field: '{city_field}'");

        // Split by first space to separate postal code from city name
        let parts: Vec<&str> = city_field.splitn(2, ' ').collect();

        if parts.len() < 2 {
            // If no space found, treat the whole thing as city name with empty postal code
            warn!("City field '{city_field}' doesn't contain postal code, using as city name only");
            return Ok(("".to_string(), city_field.to_string()));
        }

        let zip = parts[0].trim().to_string();
        let city = parts[1].trim().to_string();

        debug!("Parsed city field: zip='{zip}', city='{city}'");
        Ok((zip, city))
    }

    fn parse_card_line(&self, line: &str) -> Result<CardRecord> {
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
        let (set_name, collector_number, rarity, condition, language) =
            self.parse_set_info(&set_info);
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

    fn parse_set_info(&self, set_info: &str) -> (String, String, String, String, String) {
        let parts: Vec<&str> = set_info.split(" - ").collect();

        let set_name = parts.first().unwrap_or(&"Unknown").trim().to_string();
        let collector_number = parts.get(1).unwrap_or(&"").trim().to_string();
        let rarity = parts.get(2).unwrap_or(&"").trim().to_string();
        let condition = parts.get(3).unwrap_or(&"NM").trim().to_string();
        let language = parts.get(4).unwrap_or(&"English").trim().to_string();

        (set_name, collector_number, rarity, condition, language)
    }

    fn card_to_order(&self, card: CardRecord) -> OrderRecord {
        use crate::models::OrderItem;

        debug!("Converting card to order: {}", card.card_name);

        let item = OrderItem {
            description: format!(
                "{} - {} - {}",
                card.card_name, card.set_name, card.condition
            ),
            product_id: card.product_id.clone(),
            localized_product_name: card.card_name.clone(),
            price: self.parse_price(&card.price).unwrap_or(0.0),
            quantity: 1, // Default to 1 for card-based orders
        };

        OrderRecord {
            order_id: card.product_id.clone(),
            username: "Card Inventory".to_string(),
            name: "Card Customer".to_string(),
            street: "".to_string(),
            zip: "".to_string(),
            city: "".to_string(),
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

    pub fn validate_orders(&self, orders: &[OrderRecord]) -> Vec<String> {
        info!("Starting validation of {} orders", orders.len());
        let mut errors = Vec::new();

        for (index, order) in orders.iter().enumerate() {
            let line_num = index + 2; // +2 because CSV is 1-indexed and has header

            if order.name.trim().is_empty() {
                let error_msg = format!("Line {line_num}: Customer name is empty");
                warn!("{error_msg}");
                errors.push(error_msg);
            }

            // Street and City can be empty for some orders, make them warnings instead
            if order.street.trim().is_empty() {
                debug!("Line {line_num}: Street address is empty (this may be acceptable)");
            }

            if order.city.trim().is_empty() {
                debug!("Line {line_num}: City is empty (this may be acceptable)");
            }

            if order.country.trim().is_empty() {
                let error_msg = format!("Line {line_num}: Country is empty");
                warn!("{error_msg}");
                errors.push(error_msg);
            }

            if order.total_value.trim().is_empty() {
                let error_msg = format!("Line {line_num}: Total value is empty");
                warn!("{error_msg}");
                errors.push(error_msg);
            } else if self.parse_price(&order.total_value).is_err() {
                // More lenient price parsing - accept comma as decimal separator
                let normalized_price = order.total_value.replace(',', ".");
                if normalized_price.parse::<f64>().is_err() {
                    let error_msg = format!(
                        "Line {}: Invalid total value format: {}",
                        line_num, order.total_value
                    );
                    warn!("{error_msg}");
                    errors.push(error_msg);
                } else {
                    debug!(
                        "Line {}: Price format acceptable after normalization: {} -> {}",
                        line_num, order.total_value, normalized_price
                    );
                }
            }

            if order.currency.trim().is_empty() {
                let error_msg = format!("Line {line_num}: Currency is empty");
                warn!("{error_msg}");
                errors.push(error_msg);
            }

            if order.date_of_purchase.trim().is_empty() {
                let error_msg = format!("Line {line_num}: Purchase date is empty");
                warn!("{error_msg}");
                errors.push(error_msg);
            }
        }

        if errors.is_empty() {
            info!("All {} orders passed validation", orders.len());
        } else {
            warn!("Validation completed with {} errors", errors.len());
        }

        errors
    }

    fn parse_price(&self, price_str: &str) -> Result<f64> {
        debug!("Parsing price string: {price_str}");
        let clean_price = price_str.replace(',', ".");
        let result = clean_price.parse::<f64>().context("Failed to parse price");

        match &result {
            Ok(value) => debug!("Successfully parsed price: {value}"),
            Err(e) => warn!("Failed to parse price '{price_str}': {e}"),
        }

        result
    }
}
