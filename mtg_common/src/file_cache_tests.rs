//! Tests for file_cache.

use super::*;
use tempfile::TempDir;

#[test]
fn read_missing_returns_none() {
    let tmp = TempDir::new().unwrap();
    let cache = FileCache::new(tmp.path().to_path_buf());
    assert!(cache.read("missing.bin").is_none());
    assert!(!cache.contains("missing.bin"));
}

#[test]
fn write_then_read_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let cache = FileCache::new(tmp.path().to_path_buf());

    cache.write("a.bin", &[1, 2, 3]);

    assert!(cache.contains("a.bin"));
    assert_eq!(cache.read("a.bin").unwrap(), vec![1, 2, 3]);
}

#[test]
fn write_overwrites_existing() {
    let tmp = TempDir::new().unwrap();
    let cache = FileCache::new(tmp.path().to_path_buf());

    cache.write("a.bin", &[1, 2, 3]);
    cache.write("a.bin", &[4, 5]);

    assert_eq!(cache.read("a.bin").unwrap(), vec![4, 5]);
}

#[test]
fn entries_persist_across_instances() {
    let tmp = TempDir::new().unwrap();
    FileCache::new(tmp.path().to_path_buf()).write("a.bin", &[9]);

    let reopened = FileCache::new(tmp.path().to_path_buf());
    assert_eq!(reopened.read("a.bin").unwrap(), vec![9]);
}

#[test]
fn empty_entry_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let cache = FileCache::new(tmp.path().to_path_buf());

    cache.write("empty.bin", &[]);

    assert!(cache.read("empty.bin").unwrap().is_empty());
}
