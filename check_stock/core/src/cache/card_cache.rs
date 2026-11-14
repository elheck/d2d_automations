use crate::api::scryfall::{fetch_card, ScryfallCard};
use crate::error::ApiResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Persistent cache for Scryfall card lookups
/// Stores cards in a JSON file to avoid redundant API calls.
///
/// `CardCache::default()` is in-memory only: [`CardCache::save`] does nothing
/// until the cache was loaded from a file.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CardCache {
    /// Map of "set/collector_number" to card data
    cards: HashMap<String, ScryfallCard>,
    /// File the cache was loaded from and saves to (not part of the file).
    #[serde(skip)]
    file: Option<PathBuf>,
}

impl CardCache {
    /// File name of this cache inside the application's cache root.
    pub const FILE_NAME: &'static str = "scryfall_cache.json";

    /// Loads the cache file inside the application's cache root
    /// (`<root>/scryfall_cache.json`).
    pub fn load_in(cache_root: &Path) -> Self {
        Self::load_from(cache_root.join(Self::FILE_NAME))
    }

    /// Load cache from `path`, or start empty if it is missing or unreadable.
    /// Either way, later saves go to `path`.
    pub fn load_from(path: PathBuf) -> Self {
        let mut cache = Self::read_file(&path);
        cache.file = Some(path);
        cache
    }

    fn read_file(path: &Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
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

    /// Save cache to the file it was loaded from (no-op for an in-memory cache).
    pub fn save(&self) -> ApiResult<()> {
        let Some(path) = &self.file else {
            return Ok(());
        };

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;

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
    pub fn len(&self) -> usize {
        self.cards.len()
    }

    /// Check if empty
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
#[path = "card_cache_tests.rs"]
mod tests;
