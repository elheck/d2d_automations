//! Error types for inventory_sync

use std::fmt;

/// Unified error type for inventory_sync operations
#[derive(Debug)]
pub enum Error {
    /// HTTP request failed (network error, timeout, etc.)
    Network(reqwest::Error),
    /// Failed to parse JSON response
    Parse(serde_json::Error),
    /// HTTP error status code
    HttpStatus(reqwest::StatusCode),
    /// Database operation failed
    Database(rusqlite::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Network(e) => write!(f, "Network error: {}", e),
            Error::Parse(e) => write!(f, "Parse error: {}", e),
            Error::HttpStatus(status) => write!(f, "HTTP error: {}", status),
            Error::Database(e) => write!(f, "Database error: {}", e),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Network(e) => Some(e),
            Error::Parse(e) => Some(e),
            Error::HttpStatus(_) => None,
            Error::Database(e) => Some(e),
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Network(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Parse(err)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        Error::Database(err)
    }
}

/// Result alias for inventory_sync operations
pub type Result<T> = std::result::Result<T, Error>;
