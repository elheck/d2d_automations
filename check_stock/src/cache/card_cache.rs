use crate::api::scryfall::{fetch_card, ScryfallCard};
use crate::error::ApiResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Persistent cache for Scryfall card lookups
/// Stores cards in a JSON file to avoid redundant API calls
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CardCache {
    /// Map of "set/collector_number" to card data
    cards: HashMap<String, ScryfallCard>,
}

impl CardCache {
    /// Get the default cache file path
    fn cache_path() -> std::path::PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("d2d_automations")
            .join("scryfall_cache.json")
    }

    /// Load cache from disk, or create empty if doesn't exist
    pub fn load() -> Self {
        let path = Self::cache_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(cache) => {
                        log::info!("Loaded card cache with {} entries", Self::count(&cache));
                        return cache;
                    }
                    Err(e) => {
                        log::warn!("Failed to parse cache file, starting fresh: {}", e);
                    }
                },
                Err(e) => {
                    log::warn!("Failed to read cache file, starting fresh: {}", e);
                }
            }
        }
        log::info!("Starting with empty card cache");
        Self::default()
    }

    fn count(cache: &Self) -> usize {
        cache.cards.len()
    }

    /// Save cache to disk
    pub fn save(&self) -> ApiResult<()> {
        let path = Self::cache_path();

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;

        log::debug!("Saved card cache with {} entries", self.cards.len());
        Ok(())
    }

    /// Generate cache key from set code and collector number
    fn key(set_code: &str, collector_number: &str) -> String {
        format!("{}/{}", set_code.to_lowercase(), collector_number)
    }

    /// Get a card from cache
    pub fn get(&self, set_code: &str, collector_number: &str) -> Option<&ScryfallCard> {
        self.cards.get(&Self::key(set_code, collector_number))
    }

    /// Insert a card into cache
    pub fn insert(&mut self, set_code: &str, collector_number: &str, card: ScryfallCard) {
        self.cards
            .insert(Self::key(set_code, collector_number), card);
    }

    /// Get card count
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.cards.len()
    }

    /// Check if empty
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }
}

/// Fetch a card, checking cache first
pub fn fetch_card_cached(
    cache: &mut CardCache,
    set_code: &str,
    collector_number: &str,
) -> ApiResult<ScryfallCard> {
    // Check cache first
    if let Some(card) = cache.get(set_code, collector_number) {
        log::info!("Cache hit for {}/{}", set_code, collector_number);
        return Ok(card.clone());
    }

    // Fetch from API
    log::info!(
        "Cache miss for {}/{}, fetching from Scryfall",
        set_code,
        collector_number
    );
    let card = fetch_card(set_code, collector_number)?;

    // Store in cache and save
    cache.insert(set_code, collector_number, card.clone());
    if let Err(e) = cache.save() {
        log::warn!("Failed to save cache: {}", e);
    }

    Ok(card)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_card(name: &str, set: &str, collector_number: &str) -> ScryfallCard {
        ScryfallCard {
            id: "test-id".to_string(),
            name: name.to_string(),
            set: set.to_string(),
            set_name: "Test Set".to_string(),
            collector_number: collector_number.to_string(),
            rarity: "common".to_string(),
            prices: Default::default(),
            image_uris: None,
            card_faces: None,
            cardmarket_id: None,
            mana_cost: None,
            type_line: None,
            oracle_text: None,
        }
    }

    #[test]
    fn test_key_lowercase() {
        assert_eq!(CardCache::key("LEA", "123"), "lea/123");
        assert_eq!(CardCache::key("Hou", "45"), "hou/45");
        assert_eq!(CardCache::key("abc", "1"), "abc/1");
    }

    #[test]
    fn test_default_cache_is_empty() {
        let cache = CardCache::default();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_insert_and_get() {
        let mut cache = CardCache::default();
        let card = create_test_card("Lightning Bolt", "lea", "123");

        cache.insert("LEA", "123", card.clone());

        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());

        let retrieved = cache.get("LEA", "123");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Lightning Bolt");
    }

    #[test]
    fn test_get_case_insensitive_set_code() {
        let mut cache = CardCache::default();
        let card = create_test_card("Black Lotus", "lea", "456");

        cache.insert("LEA", "456", card);

        // Should find with different cases
        assert!(cache.get("lea", "456").is_some());
        assert!(cache.get("LEA", "456").is_some());
        assert!(cache.get("Lea", "456").is_some());
    }

    #[test]
    fn test_get_nonexistent_returns_none() {
        let cache = CardCache::default();
        assert!(cache.get("lea", "999").is_none());
    }

    #[test]
    fn test_insert_overwrites_existing() {
        let mut cache = CardCache::default();
        let card1 = create_test_card("First Card", "lea", "100");
        let card2 = create_test_card("Second Card", "lea", "100");

        cache.insert("lea", "100", card1);
        cache.insert("lea", "100", card2);

        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get("lea", "100").unwrap().name, "Second Card");
    }

    #[test]
    fn test_multiple_cards() {
        let mut cache = CardCache::default();

        cache.insert("lea", "1", create_test_card("Card 1", "lea", "1"));
        cache.insert("lea", "2", create_test_card("Card 2", "lea", "2"));
        cache.insert("hou", "1", create_test_card("Card 3", "hou", "1"));

        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get("lea", "1").unwrap().name, "Card 1");
        assert_eq!(cache.get("lea", "2").unwrap().name, "Card 2");
        assert_eq!(cache.get("hou", "1").unwrap().name, "Card 3");
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let cache_file = temp_dir.path().join("test_cache.json");

        // Create and populate cache
        let mut cache = CardCache::default();
        cache.insert(
            "lea",
            "123",
            create_test_card("Lightning Bolt", "lea", "123"),
        );
        cache.insert("hou", "45", create_test_card("Counterspell", "hou", "45"));

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&cache).unwrap();
        std::fs::write(&cache_file, &json).unwrap();

        // Read back and deserialize
        let loaded_json = std::fs::read_to_string(&cache_file).unwrap();
        let loaded_cache: CardCache = serde_json::from_str(&loaded_json).unwrap();

        assert_eq!(loaded_cache.len(), 2);
        assert_eq!(
            loaded_cache.get("lea", "123").unwrap().name,
            "Lightning Bolt"
        );
        assert_eq!(loaded_cache.get("hou", "45").unwrap().name, "Counterspell");
    }

    #[test]
    fn test_serialization_format() {
        let mut cache = CardCache::default();
        cache.insert("lea", "1", create_test_card("Test Card", "lea", "1"));

        let json = serde_json::to_string(&cache).unwrap();

        // Verify the key format in JSON
        assert!(json.contains("\"lea/1\""));
    }
}
