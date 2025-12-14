use std::fmt;

/// Unified error type for API and I/O operations
#[derive(Debug)]
#[allow(dead_code)]
pub enum ApiError {
    /// HTTP request failed (network error, timeout, etc.)
    Network(reqwest::Error),
    /// Failed to parse JSON response
    Parse(serde_json::Error),
    /// API returned an error response
    ApiResponse { code: String, details: String },
    /// HTTP error status code
    HttpStatus(reqwest::StatusCode),
    /// File I/O error
    Io(std::io::Error),
    /// Image decoding error
    Image(String),
    /// Cache operation failed
    Cache(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Network(e) => write!(f, "Network error: {}", e),
            ApiError::Parse(e) => write!(f, "Parse error: {}", e),
            ApiError::ApiResponse { code, details } => write!(f, "{}: {}", code, details),
            ApiError::HttpStatus(status) => write!(f, "HTTP error: {}", status),
            ApiError::Io(e) => write!(f, "I/O error: {}", e),
            ApiError::Image(msg) => write!(f, "Image error: {}", msg),
            ApiError::Cache(msg) => write!(f, "Cache error: {}", msg),
        }
    }
}

impl std::error::Error for ApiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ApiError::Network(e) => Some(e),
            ApiError::Parse(e) => Some(e),
            ApiError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(err: reqwest::Error) -> Self {
        ApiError::Network(err)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        ApiError::Parse(err)
    }
}

impl From<std::io::Error> for ApiError {
    fn from(err: std::io::Error) -> Self {
        ApiError::Io(err)
    }
}

/// Result type alias for API operations
pub type ApiResult<T> = Result<T, ApiError>;
