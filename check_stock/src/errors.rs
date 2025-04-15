use std::fmt;

#[derive(Debug)]
pub enum Error {
    ConfigError(String),
    RuntimeError(String),
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
            Error::RuntimeError(msg) => write!(f, "Runtime error: {}", msg),
        }
    }
}