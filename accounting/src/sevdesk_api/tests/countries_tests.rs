//! Tests for country cache functionality.

use crate::sevdesk_api::countries::CountryCache;

#[test]
fn country_cache_default_is_not_loaded() {
    let cache = CountryCache::default();
    assert!(!cache.loaded);
    assert!(cache.name_to_id.is_empty());
}
