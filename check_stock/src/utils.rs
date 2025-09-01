use std::path::Path;

/// Checks if a path exists
pub fn path_exists<P: AsRef<Path>>(path: P) -> bool {
    Path::new(path.as_ref()).exists()
}

pub mod time {
    use chrono::Utc;

    pub fn get_current_time() -> String {
        Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
    }
}
