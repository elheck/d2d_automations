//! SevDesk API client module for invoice creation and management.
//!
//! This module provides functionality to interact with the SevDesk API,
//! including creating invoices, managing contacts, and handling country lookups.

mod check_accounts;
mod client;
mod contacts;
mod countries;
mod invoice_workflow;
mod invoices;
mod simulation;
mod users;
mod utils;

use std::sync::Arc;
use tokio::sync::RwLock;

use reqwest::Client;

use countries::CountryCache;

/// SevDesk API client for creating invoices and managing contacts.
pub struct SevDeskApi {
    pub(crate) client: Client,
    pub(crate) api_token: String,
    pub(crate) base_url: String,
    pub(crate) country_cache: Arc<RwLock<CountryCache>>,
}

impl SevDeskApi {
    /// Creates a new SevDesk API client with the given API token.
    pub fn new(api_token: String) -> Self {
        log::info!("Creating SevDesk API client");
        log::debug!("API token length: {}", api_token.len());
        Self {
            client: Client::new(),
            api_token,
            base_url: "https://my.sevdesk.de/api/v1".to_string(),
            country_cache: Arc::new(RwLock::new(CountryCache::default())),
        }
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
