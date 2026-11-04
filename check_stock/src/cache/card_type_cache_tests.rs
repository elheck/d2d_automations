//! Tests for the card type cache.

use super::*;
use chrono::NaiveDate;

fn day(d: &str) -> NaiveDate {
    NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()
}

#[test]
fn types_survive_a_reload() {
    let dir = tempfile::tempdir().unwrap();
    let mut cache = CardTypeCache::with_dir(dir.path().to_path_buf());
    assert!(cache.is_empty());
    cache.insert("MH3", "12", "Land");
    cache.save();

    let reloaded = CardTypeCache::with_dir(dir.path().to_path_buf());
    assert_eq!(reloaded.type_line("mh3", " 12 "), Some("Land"));
    assert_eq!(reloaded.len(), 1);
}

#[test]
fn missing_printings_are_rechecked_after_a_while() {
    let dir = tempfile::tempdir().unwrap();
    let mut cache = CardTypeCache::with_dir(dir.path().to_path_buf());
    cache.insert_missing("XMKM", "1", day("2026-11-01"));
    assert!(cache.is_settled("xmkm", "1", day("2026-11-04")));
    assert!(!cache.is_settled("xmkm", "1", day("2026-12-15")));
    assert_eq!(cache.type_line("xmkm", "1"), None);

    // A later hit replaces the miss.
    cache.insert("XMKM", "1", "Creature — Elf");
    assert!(cache.is_settled("xmkm", "1", day("2027-06-01")));
}

#[test]
fn unknown_printings_are_not_settled() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CardTypeCache::with_dir(dir.path().to_path_buf());
    assert!(!cache.is_settled("tst", "1", day("2026-11-04")));
}

#[test]
fn bogus_values_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let mut cache = CardTypeCache::with_dir(dir.path().to_path_buf());
    cache.insert("tst", "1", "   ");
    cache.insert("tst", "2", &"x".repeat(MAX_TYPE_LINE_CHARS + 1));
    assert!(cache.is_empty());

    // Oversized entries planted in the file are dropped on load.
    let planted = serde_json::json!({
        "schema": TYPE_CACHE_SCHEMA,
        "types": { "tst/3": "x".repeat(MAX_TYPE_LINE_CHARS + 1), "tst/4": "Land" },
        "missing": {}
    });
    std::fs::write(dir.path().join(TYPES_FILE), planted.to_string()).unwrap();
    let loaded = CardTypeCache::with_dir(dir.path().to_path_buf());
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded.type_line("tst", "4"), Some("Land"));
}

#[test]
fn unreadable_or_outdated_files_start_empty() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(TYPES_FILE), b"not json").unwrap();
    assert!(CardTypeCache::with_dir(dir.path().to_path_buf()).is_empty());

    let old = serde_json::json!({ "schema": 0, "types": { "tst/1": "Land" }, "missing": {} });
    std::fs::write(dir.path().join(TYPES_FILE), old.to_string()).unwrap();
    assert!(CardTypeCache::with_dir(dir.path().to_path_buf()).is_empty());
}

#[test]
fn save_without_changes_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut cache = CardTypeCache::with_dir(dir.path().to_path_buf());
    cache.save();
    assert!(!dir.path().join(TYPES_FILE).exists());
}
