use super::*;
use crate::api::mtgjson::{DeckBoard, DeckCard};

fn deck(mcm_id: Option<u64>) -> PreconDeck {
    PreconDeck {
        code: "BLC".to_string(),
        name: "Animated Army".to_string(),
        release_date: Some("2024-08-02".to_string()),
        cards: vec![DeckCard {
            name: "Sol Ring".to_string(),
            set_code: "BLC".to_string(),
            number: "1".to_string(),
            count: 1,
            is_foil: false,
            mcm_id,
            board: DeckBoard::Main,
        }],
    }
}

fn cached(fetched_at_unix: i64, seen: Vec<u64>) -> CachedIndex {
    CachedIndex {
        schema: INDEX_SCHEMA,
        fetched_at_unix,
        seen_catalog_ids: seen,
        index: SealedIndex::default(),
    }
}

fn dir_entries(path: &std::path::Path) -> usize {
    std::fs::read_dir(path).map(|d| d.count()).unwrap_or(0)
}

#[test]
fn deck_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = DeckCache::with_dir(tmp.path().to_path_buf());
    assert!(cache.get("AnimatedArmy_BLC").is_none());
    assert!(cache.insert("AnimatedArmy_BLC", &deck(Some(1))));
    assert_eq!(cache.get("AnimatedArmy_BLC"), Some(deck(Some(1))));
    assert!(cache.deck_dir().join("AnimatedArmy_BLC.json").exists());
}

#[test]
fn incomplete_decks_are_not_stored() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = DeckCache::with_dir(tmp.path().to_path_buf());
    assert!(!cache.insert("AnimatedArmy_BLC", &deck(None)));
    assert_eq!(dir_entries(cache.deck_dir()), 0);
}

#[test]
fn unsafe_names_are_refused_on_read_and_write() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = DeckCache::with_dir(tmp.path().join("cache"));
    // Plant a file outside the deck dir that a traversal would reach.
    std::fs::write(
        tmp.path().join("cache").join("evil.json"),
        serde_json::to_vec(&deck(Some(1))).unwrap(),
    )
    .unwrap();
    assert!(cache.get("../evil").is_none());
    assert!(!cache.insert("../evil2", &deck(Some(1))));
    assert!(!cache.insert("a.b", &deck(Some(1))));
    assert_eq!(dir_entries(cache.deck_dir()), 0);
    assert!(!tmp.path().join("cache").join("evil2.json").exists());
}

#[test]
fn corrupt_or_invalid_cached_decks_are_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = DeckCache::with_dir(tmp.path().to_path_buf());
    std::fs::write(cache.deck_dir().join("Truncated_X.json"), b"{\"code\":").unwrap();
    assert!(cache.get("Truncated_X").is_none());

    let mut bad = deck(Some(1));
    bad.cards[0].count = 100_000;
    std::fs::write(
        cache.deck_dir().join("Tampered_X.json"),
        serde_json::to_vec(&bad).unwrap(),
    )
    .unwrap();
    assert!(cache.get("Tampered_X").is_none());
}

#[test]
fn index_round_trip_and_corruption() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = DeckCache::with_dir(tmp.path().to_path_buf());
    assert!(cache.load_index().is_none());
    let index = cached(1_000, vec![1, 2]);
    cache.store_index(&index);
    assert_eq!(cache.load_index(), Some(index));

    std::fs::write(tmp.path().join("sealed_index.json"), b"not json").unwrap();
    assert!(cache.load_index().is_none());
}

#[test]
fn needs_rebuild_rules() {
    let now = 10_000_000;
    let fresh = cached(now - 60, vec![1, 2, 3]);
    assert!(needs_rebuild(None, &[1], now));
    assert!(!needs_rebuild(Some(&fresh), &[1, 2], now));
    assert!(!needs_rebuild(Some(&fresh), &[], now));
    // A catalog product the index has never seen.
    assert!(needs_rebuild(Some(&fresh), &[1, 4], now));
    // Older than 7 days.
    let old = cached(now - INDEX_MAX_AGE_SECS - 1, vec![1]);
    assert!(needs_rebuild(Some(&old), &[1], now));
    // Exactly 7 days is still fresh.
    let edge = cached(now - INDEX_MAX_AGE_SECS, vec![1]);
    assert!(!needs_rebuild(Some(&edge), &[1], now));
    // Timestamp far in the future (clock skew or tampering).
    let future = cached(now + 2 * 24 * 60 * 60, vec![1]);
    assert!(needs_rebuild(Some(&future), &[1], now));
    // Different schema.
    let mut other = cached(now, vec![1]);
    other.schema = INDEX_SCHEMA + 1;
    assert!(needs_rebuild(Some(&other), &[1], now));
}
