//! Tests for image_cache.

use super::*;

#[test]
fn test_insert_and_get_image() {
    use tempfile::TempDir;
    let temp_dir = TempDir::new().unwrap();
    let cache = ImageCache::new(temp_dir.path());
    let test_data = vec![0xFF, 0xD8, 0xFF]; // JPEG magic bytes

    assert!(!cache.contains_image(12345));

    cache.insert_image(12345, &test_data);

    assert!(cache.contains_image(12345));
    let retrieved = cache.get_image(12345);
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap(), test_data);
}

#[test]
fn test_metadata_cache() {
    use tempfile::TempDir;
    let temp_dir = TempDir::new().unwrap();
    let cache = ImageCache::new(temp_dir.path());

    // No metadata initially
    assert!(cache.get_meta(752712).is_none());

    let info = CardInfo {
        set_name: Some("Ravnica: Clue Edition".to_string()),
        type_line: Some("Instant".to_string()),
        mana_cost: Some("{R}".to_string()),
        rarity: Some("uncommon".to_string()),
        oracle_text: Some("Deals 3 damage.".to_string()),
        purchase_uris: None,
    };

    cache.insert_meta(752712, &info);

    let retrieved = cache.get_meta(752712);
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.set_name.as_deref(), Some("Ravnica: Clue Edition"));
    assert_eq!(retrieved.type_line.as_deref(), Some("Instant"));
    assert_eq!(retrieved.mana_cost.as_deref(), Some("{R}"));
    assert_eq!(retrieved.rarity.as_deref(), Some("uncommon"));
}

#[test]
fn test_different_products_cached_separately() {
    use tempfile::TempDir;
    let temp_dir = TempDir::new().unwrap();
    let cache = ImageCache::new(temp_dir.path());

    let info_a = CardInfo {
        set_name: Some("Alpha".to_string()),
        type_line: None,
        mana_cost: None,
        rarity: None,
        oracle_text: None,
        purchase_uris: None,
    };
    let info_b = CardInfo {
        set_name: Some("Beta".to_string()),
        type_line: None,
        mana_cost: None,
        rarity: None,
        oracle_text: None,
        purchase_uris: None,
    };

    cache.insert_meta(100, &info_a);
    cache.insert_meta(200, &info_b);

    assert_eq!(
        cache.get_meta(100).unwrap().set_name.as_deref(),
        Some("Alpha")
    );
    assert_eq!(
        cache.get_meta(200).unwrap().set_name.as_deref(),
        Some("Beta")
    );
}

#[test]
fn misses_are_remembered_for_thirty_days() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache = ImageCache::new(temp.path());
    let now = SystemTime::now();
    assert!(!cache.is_recent_miss(42, now));
    cache.record_miss(42, now);
    assert!(cache.is_recent_miss(42, now));
    assert!(cache.is_recent_miss(42, now + MISS_TTL - Duration::from_secs(1)));
    assert!(!cache.is_recent_miss(42, now + MISS_TTL));
    assert!(!cache.is_recent_miss(43, now), "per product");
}

#[test]
fn a_damaged_miss_marker_counts_as_no_miss() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache = ImageCache::new(temp.path());
    std::fs::create_dir_all(temp.path().join("card_images")).unwrap();
    std::fs::write(temp.path().join("card_images/7.miss"), "garbage").unwrap();
    assert!(!cache.is_recent_miss(7, SystemTime::now()));
}

#[test]
fn only_not_found_errors_are_remembered() {
    assert!(is_permanent_miss(&InventoryError::ScryfallNotFound(
        "x".into()
    )));
    assert!(is_permanent_miss(&InventoryError::NoImageAvailable(
        "x".into()
    )));
    assert!(!is_permanent_miss(&InventoryError::ImageFetchFailed(
        "x".into()
    )));
    assert!(!is_permanent_miss(&InventoryError::HttpStatus(
        reqwest::StatusCode::BAD_GATEWAY
    )));
}
