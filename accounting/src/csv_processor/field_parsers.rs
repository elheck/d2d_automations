//! Field parsing utilities for CSV data.
//!
//! Contains pure functions for parsing individual fields like prices,
//! cities, quantities, etc.

use anyhow::{Context, Result};
use log::{debug, warn};

/// Parses a price string, handling both comma and dot decimal separators.
///
/// # Arguments
/// * `price_str` - A string containing a price value (e.g., "5,00" or "5.00")
///
/// # Returns
/// The parsed price as f64, or an error if parsing fails.
pub fn parse_price(price_str: &str) -> Result<f64> {
    debug!("Parsing price string: {price_str}");
    let clean_price = price_str.replace(',', ".");
    let result = clean_price.parse::<f64>().context("Failed to parse price");

    match &result {
        Ok(value) => debug!("Successfully parsed price: {value}"),
        Err(e) => warn!("Failed to parse price '{price_str}': {e}"),
    }

    result
}

/// Parses a city field that contains both postal code and city name.
///
/// # Arguments
/// * `city_field` - A string like "10557 Berlin" or "SW1A London"
///
/// # Returns
/// A tuple of (zip_code, city_name). If no space is found, returns empty zip
/// and the full string as city name.
pub fn parse_city_field(city_field: &str) -> Result<(String, String)> {
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

/// Extracts quantity from a description string.
///
/// Looks for patterns like "2x", "10x" at the beginning of the description.
///
/// # Arguments
/// * `description` - A description string like "2x High Fae Trickster - 1,87 EUR"
///
/// # Returns
/// The extracted quantity, or 1 if no quantity pattern is found.
pub fn extract_quantity_from_description(description: &str) -> u32 {
    debug!("Extracting quantity from description: {description}");

    // Look for patterns like "2x", "10x", etc. at the beginning of the description
    if let Some(first_part) = description.split_whitespace().next() {
        if let Some(stripped) = first_part.strip_suffix('x') {
            if let Ok(quantity) = stripped.parse::<u32>() {
                debug!("Found quantity: {quantity}");
                return quantity;
            }
        }
    }

    debug!("No quantity found, defaulting to 1");
    1 // Default to 1 if no quantity is found
}

/// Extracts price from a description string.
///
/// Looks for patterns like "- 0,19 EUR" or "- 5,35 EUR" at the end of the description.
///
/// # Arguments
/// * `description` - A description string like "1x Card Name - 5,00 EUR"
///
/// # Returns
/// The extracted price as f64, or an error if no price pattern is found.
pub fn extract_price_from_description(description: &str) -> Result<f64> {
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

#[cfg(test)]
#[path = "field_parsers_tests.rs"]
mod tests;
