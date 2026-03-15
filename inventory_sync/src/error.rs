//! Error types for inventory_sync

use std::fmt;

/// Unified error type for inventory_sync operations
#[derive(Debug)]
pub enum InventoryError {
    /// HTTP request failed (network error, timeout, etc.)
    Network(reqwest::Error),
    /// Failed to parse JSON response
    Parse(serde_json::Error),
    /// HTTP error status code
    HttpStatus(reqwest::StatusCode),
    /// Database operation failed
    Database(rusqlite::Error),
    /// Card not found on Scryfall
    ScryfallNotFound(String),
    /// No image available for card
    NoImageAvailable(String),
    /// Failed to fetch image from URL
    ImageFetchFailed(String),
}

/// Legacy alias for backwards compatibility
pub type Error = InventoryError;

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InventoryError::Network(e) => write!(f, "Network error: {}", e),
            InventoryError::Parse(e) => write!(f, "Parse error: {}", e),
            InventoryError::HttpStatus(status) => write!(f, "HTTP error: {}", status),
            InventoryError::Database(e) => write!(f, "Database error: {}", e),
            InventoryError::ScryfallNotFound(name) => {
                write!(f, "Card not found on Scryfall: {}", name)
            }
            InventoryError::NoImageAvailable(name) => {
                write!(f, "No image available for card: {}", name)
            }
            InventoryError::ImageFetchFailed(url) => {
                write!(f, "Failed to fetch image from: {}", url)
            }
        }
    }
}

impl std::error::Error for InventoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InventoryError::Network(e) => Some(e),
            InventoryError::Parse(e) => Some(e),
            InventoryError::HttpStatus(_) => None,
            InventoryError::Database(e) => Some(e),
            InventoryError::ScryfallNotFound(_) => None,
            InventoryError::NoImageAvailable(_) => None,
            InventoryError::ImageFetchFailed(_) => None,
        }
    }
}

impl From<reqwest::Error> for InventoryError {
    fn from(err: reqwest::Error) -> Self {
        InventoryError::Network(err)
    }
}

impl From<serde_json::Error> for InventoryError {
    fn from(err: serde_json::Error) -> Self {
        InventoryError::Parse(err)
    }
}

impl From<rusqlite::Error> for InventoryError {
    fn from(err: rusqlite::Error) -> Self {
        InventoryError::Database(err)
    }
}

impl From<mtg_common::MtgError> for InventoryError {
    fn from(err: mtg_common::MtgError) -> Self {
        match err {
            mtg_common::MtgError::Network(e) => InventoryError::Network(e),
            mtg_common::MtgError::Parse(e) => InventoryError::Parse(e),
            mtg_common::MtgError::HttpStatus(s) => InventoryError::HttpStatus(s),
            mtg_common::MtgError::Io(_) => {
                InventoryError::ImageFetchFailed("I/O error".to_string())
            }
        }
    }
}

/// Result alias for inventory_sync operations
pub type Result<T> = std::result::Result<T, InventoryError>;
