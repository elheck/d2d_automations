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
#[path = "validator_tests.rs"]
mod tests;
