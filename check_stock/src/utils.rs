use std::path::Path;

/// Checks if a path exists
pub fn path_exists<P: AsRef<Path>>(path: P) -> bool {
    Path::new(path.as_ref()).exists()
}

/// Helper function to get the current timestamp
pub fn get_timestamp() -> String {
    use chrono::Utc;
    Utc::now().to_rfc3339()
}