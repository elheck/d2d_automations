//! CSV processing module for order and card data.
//!
//! This module provides functionality for parsing CSV files exported from Cardmarket,
//! including both order data and card inventory formats.
//!
//! # Module Structure
//!
//! - [`field_parsers`] - Pure parsing utility functions for prices, cities, and item details
//! - [`order_parser`] - Order CSV parsing (Cardmarket export format)
//! - [`card_parser`] - Card/inventory data parsing
//! - [`validator`] - Order validation logic
//!
//! # Example
//!
//! ```no_run
//! use sevdesk_invoicing::csv_processor::CsvProcessor;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let processor = CsvProcessor::new();
//!     let orders = processor.load_orders_from_csv("orders.csv").await?;
//!     let errors = processor.validate_orders(&orders);
//!     
//!     if errors.is_empty() {
//!         println!("All {} orders are valid!", orders.len());
//!     }
//!     Ok(())
//! }
//! ```

pub mod card_parser;
pub mod field_parsers;
pub mod order_parser;
pub mod validator;

use anyhow::{Context, Result};
use log::{debug, info, warn};
use std::path::Path;

use crate::models::OrderRecord;

/// CSV processor for Cardmarket order and inventory data.
///
/// Provides a unified interface for loading and validating CSV data,
/// automatically detecting the format (order CSV vs card data).
#[derive(Default)]
pub struct CsvProcessor;

impl CsvProcessor {
    /// Creates a new CSV processor instance.
    pub fn new() -> Self {
        debug!("Creating new CSV processor");
        Self
    }

    /// Loads orders from a CSV file.
    ///
    /// Automatically detects whether the file contains:
    /// - Order data (with headers like OrderID, Username, Name)
    /// - Card inventory data (tabular format with prices and product IDs)
    ///
    /// # Arguments
    /// * `file_path` - Path to the CSV file
    ///
    /// # Returns
    /// A vector of parsed OrderRecord, or an error if the file cannot be read or parsed.
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

    /// Parses CSV content, auto-detecting the format.
    ///
    /// # Arguments
    /// * `content` - Raw CSV content as a string
    ///
    /// # Returns
    /// A vector of parsed OrderRecord.
    pub fn parse_csv_content(&self, content: &str) -> Result<Vec<OrderRecord>> {
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
            return order_parser::parse_csv_with_headers(content);
        }

        // Otherwise try to parse as card data
        info!("Attempting to parse as card data format");
        for (line_num, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                debug!("Skipping empty line {}", line_num + 1);
                continue;
            }

            match card_parser::parse_card_line(line) {
                Ok(card) => {
                    debug!("Successfully parsed card: {}", card.card_name);
                    orders.push(card_parser::card_to_order(card));
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

    /// Validates a collection of orders.
    ///
    /// Checks for required fields and valid data formats.
    ///
    /// # Arguments
    /// * `orders` - Slice of orders to validate
    ///
    /// # Returns
    /// A vector of error messages. Empty if all orders are valid.
    pub fn validate_orders(&self, orders: &[OrderRecord]) -> Vec<String> {
        validator::validate_orders(orders)
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
