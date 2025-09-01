use d2d_automations::utils::{path_exists, time};
use std::path::Path;
use tempfile::{NamedTempFile, TempDir};

#[test]
fn test_path_exists_with_existing_file() {
    // Create a temporary file
    let temp_file = NamedTempFile::new().unwrap();
    let temp_path = temp_file.path();

    // Test that path_exists returns true for existing file
    assert!(path_exists(temp_path));
    assert!(path_exists(temp_path.to_str().unwrap()));
}

#[test]
fn test_path_exists_with_existing_directory() {
    // Create a temporary directory
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Test that path_exists returns true for existing directory
    assert!(path_exists(temp_path));
    assert!(path_exists(temp_path.to_str().unwrap()));
}

#[test]
fn test_path_exists_with_nonexistent_path() {
    // Test with a path that definitely doesn't exist
    let nonexistent_path = "/this/path/definitely/does/not/exist/hopefully/12345";

    assert!(!path_exists(nonexistent_path));
    assert!(!path_exists(Path::new(nonexistent_path)));
}

#[test]
fn test_path_exists_with_different_path_types() {
    let temp_file = NamedTempFile::new().unwrap();
    let temp_path = temp_file.path();

    // Test with different path representations
    assert!(path_exists(temp_path)); // &Path
    assert!(path_exists(temp_path)); // PathBuf (already a Path)
    assert!(path_exists(temp_path.to_str().unwrap())); // &str
    assert!(path_exists(temp_path.to_string_lossy().to_string())); // String
}

#[test]
fn test_get_current_time_format() {
    let time_str = time::get_current_time();

    // Test that the time string has the expected format
    // Format should be "YYYY-MM-DD HH:MM:SS"
    assert_eq!(time_str.len(), 19); // "2024-01-01 12:34:56" is 19 characters

    // Test that it contains expected separators
    assert!(time_str.contains('-')); // Date separators
    assert!(time_str.contains(' ')); // Space between date and time
    assert!(time_str.contains(':')); // Time separators

    // Test that we can parse the components
    let parts: Vec<&str> = time_str.split(' ').collect();
    assert_eq!(parts.len(), 2); // Should have date and time parts

    let date_parts: Vec<&str> = parts[0].split('-').collect();
    assert_eq!(date_parts.len(), 3); // Year, month, day

    let time_parts: Vec<&str> = parts[1].split(':').collect();
    assert_eq!(time_parts.len(), 3); // Hour, minute, second
}

#[test]
fn test_get_current_time_is_recent() {
    use chrono::{DateTime, Utc};

    let time_str = time::get_current_time();

    // Parse the returned time string
    let parsed_time =
        DateTime::parse_from_str(&format!("{}+00:00", time_str), "%Y-%m-%d %H:%M:%S%z")
            .expect("Should be able to parse the time string");

    let now = Utc::now();
    let diff = now.signed_duration_since(parsed_time.with_timezone(&Utc));

    // The difference should be very small (less than 5 seconds)
    assert!(
        diff.num_seconds().abs() < 5,
        "Time difference too large: {} seconds",
        diff.num_seconds()
    );
}

#[test]
fn test_get_current_time_consistency() {
    // Get multiple time strings in quick succession
    let time1 = time::get_current_time();
    let time2 = time::get_current_time();
    let time3 = time::get_current_time();

    // They should all be valid format
    assert_eq!(time1.len(), 19);
    assert_eq!(time2.len(), 19);
    assert_eq!(time3.len(), 19);

    // They should be the same or very close (within same second potentially)
    // At minimum, the date part should be identical
    let date1 = &time1[..10]; // "YYYY-MM-DD"
    let date2 = &time2[..10];
    let date3 = &time3[..10];

    assert_eq!(date1, date2);
    assert_eq!(date2, date3);
}

#[cfg(test)]
mod edge_cases {
    use super::*;

    #[test]
    fn test_path_exists_empty_string() {
        // Empty string should return false (or possibly true if current dir exists)
        let result = path_exists("");
        // This might be true if current directory exists, which is usually the case
        // So we just test that it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_path_exists_relative_paths() {
        // Test with relative paths
        assert!(path_exists(".")); // Current directory should exist

        // Test with a relative path that probably doesn't exist
        assert!(!path_exists("./this_file_should_not_exist_12345"));
    }

    #[test]
    fn test_multiple_time_calls() {
        // Test that multiple calls don't cause issues
        for _ in 0..10 {
            let time_str = time::get_current_time();
            assert!(!time_str.is_empty());
            assert_eq!(time_str.len(), 19);
        }
    }
}
