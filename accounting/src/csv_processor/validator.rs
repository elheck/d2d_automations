//! Order validation logic.
//!
//! Validates order records for completeness and correctness before
//! sending to the SevDesk API.

use log::{debug, info, warn};

use crate::models::OrderRecord;

use super::field_parsers::parse_price;

/// Validates a list of order records.
///
/// Checks for required fields and valid formats. Returns a list of
/// error messages for any validation failures.
///
/// # Arguments
/// * `orders` - Slice of OrderRecord to validate
///
/// # Returns
/// A vector of error messages. Empty if all orders are valid.
pub fn validate_orders(orders: &[OrderRecord]) -> Vec<String> {
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
        } else if parse_price(&order.total_value).is_err() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::OrderItem;

    fn create_valid_order() -> OrderRecord {
        OrderRecord {
            order_id: "12345".to_string(),
            username: "user".to_string(),
            name: "John Doe".to_string(),
            street: "Main Street 1".to_string(),
            zip: "10557".to_string(),
            city: "Berlin".to_string(),
            country: "Germany".to_string(),
            is_professional: None,
            vat_number: None,
            date_of_purchase: "2025-01-15".to_string(),
            article_count: 1,
            merchandise_value: "5,00".to_string(),
            shipment_costs: "1,50".to_string(),
            total_value: "6,50".to_string(),
            commission: "0,10".to_string(),
            currency: "EUR".to_string(),
            description: "1x Card".to_string(),
            product_id: "98765".to_string(),
            localized_product_name: "Card Name".to_string(),
            items: vec![OrderItem {
                description: "1x Card".to_string(),
                product_id: "98765".to_string(),
                localized_product_name: "Card Name".to_string(),
                price: 5.0,
                quantity: 1,
            }],
        }
    }

    #[test]
    fn validates_correct_order() {
        let orders = vec![create_valid_order()];
        let errors = validate_orders(&orders);
        assert!(errors.is_empty());
    }

    #[test]
    fn detects_empty_customer_name() {
        let mut order = create_valid_order();
        order.name = "".to_string();

        let errors = validate_orders(&[order]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Customer name is empty"));
    }

    #[test]
    fn detects_empty_country() {
        let mut order = create_valid_order();
        order.country = "".to_string();

        let errors = validate_orders(&[order]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Country is empty"));
    }

    #[test]
    fn detects_empty_total_value() {
        let mut order = create_valid_order();
        order.total_value = "".to_string();

        let errors = validate_orders(&[order]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Total value is empty"));
    }

    #[test]
    fn detects_invalid_total_value_format() {
        let mut order = create_valid_order();
        order.total_value = "not a price".to_string();

        let errors = validate_orders(&[order]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Invalid total value format"));
    }

    #[test]
    fn accepts_comma_decimal_in_total_value() {
        let mut order = create_valid_order();
        order.total_value = "6,50".to_string();

        let errors = validate_orders(&[order]);
        assert!(errors.is_empty());
    }

    #[test]
    fn detects_empty_currency() {
        let mut order = create_valid_order();
        order.currency = "".to_string();

        let errors = validate_orders(&[order]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Currency is empty"));
    }

    #[test]
    fn detects_empty_date() {
        let mut order = create_valid_order();
        order.date_of_purchase = "".to_string();

        let errors = validate_orders(&[order]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Purchase date is empty"));
    }

    #[test]
    fn detects_multiple_errors() {
        let mut order = create_valid_order();
        order.name = "".to_string();
        order.country = "".to_string();
        order.currency = "".to_string();

        let errors = validate_orders(&[order]);
        assert_eq!(errors.len(), 3);
    }

    #[test]
    fn validates_multiple_orders() {
        let mut order1 = create_valid_order();
        order1.name = "".to_string();

        let mut order2 = create_valid_order();
        order2.country = "".to_string();

        let errors = validate_orders(&[order1, order2]);
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("Line 2")); // First order
        assert!(errors[1].contains("Line 3")); // Second order
    }

    #[test]
    fn allows_empty_street_and_city() {
        let mut order = create_valid_order();
        order.street = "".to_string();
        order.city = "".to_string();

        let errors = validate_orders(&[order]);
        assert!(errors.is_empty()); // Should be warnings, not errors
    }
}
