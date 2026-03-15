/// Common error type for MTG API operations.
///
/// Contains the shared error variants used across projects.
/// Project-specific errors should wrap or convert from this type.
#[derive(Debug, thiserror::Error)]
pub enum MtgError {
    /// HTTP request failed (network error, timeout, etc.)
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    /// Failed to parse JSON response
    #[error("Parse error: {0}")]
    Parse(#[from] serde_json::Error),

    /// HTTP error status code
    #[error("HTTP error: {0}")]
    HttpStatus(reqwest::StatusCode),

    /// File I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result alias for common MTG operations.
pub type MtgResult<T> = Result<T, MtgError>;
