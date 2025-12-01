//! Utility functions for the SevDesk API.

use anyhow::{Context, Result};
use log::{debug, error};

use super::SevDeskApi;

impl SevDeskApi {
    /// Parse price string to f64, handling comma as decimal separator.
    ///
    /// # Examples
    /// - "5,00" -> 5.0
    /// - "5.00" -> 5.0
    /// - "100" -> 100.0
    pub(crate) fn parse_price(&self, price_str: &str) -> Result<f64> {
        debug!("Parsing price: '{price_str}'");
        let clean_price = price_str.replace(',', ".");
        let result = clean_price.parse::<f64>().context("Failed to parse price");

        match &result {
            Ok(price) => debug!("Parsed price '{price_str}' as {price}"),
            Err(e) => error!("Failed to parse price '{price_str}': {e}"),
        }

        result
    }
}
