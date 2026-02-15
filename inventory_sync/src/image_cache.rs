//! Persistent cache for card images
//!
//! Stores images as files in the cache directory alongside the database.
//! Uses card name as the cache key (sanitized for filesystem safety).

use crate::error::InventoryError;
use crate::scryfall::{fetch_card_by_name, fetch_image, CardInfo};
use std::path::PathBuf;

/// Persistent cache for card images
pub struct ImageCache {
    cache_dir: PathBuf,
}

impl ImageCache {
    /// Create a new image cache in the same directory as the database
    pub fn new(db_dir: &std::path::Path) -> Self {
        let cache_dir = db_dir.join("card_images");

        // Create directory if needed
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            log::warn!("Failed to create image cache directory: {}", e);
        } else {
            log::info!("Image cache directory: {:?}", cache_dir);
        }

        Self { cache_dir }
    }

    /// Sanitize card name for use as a filename
    /// Replaces invalid characters with underscores
    fn sanitize_filename(card_name: &str) -> String {
        card_name
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                c if c.is_control() => '_',
                c => c,
            })
            .collect::<String>()
            .trim()
            .to_lowercase()
    }

    /// Generate a filename from card name
    fn filename(card_name: &str) -> String {
        format!("{}.jpg", Self::sanitize_filename(card_name))
    }

    /// Get the full path for a cached image
    fn path(&self, card_name: &str) -> PathBuf {
        self.cache_dir.join(Self::filename(card_name))
    }

    /// Get the full path for cached card metadata (JSON)
    fn meta_path(&self, card_name: &str) -> PathBuf {
        self.cache_dir
            .join(format!("{}.json", Self::sanitize_filename(card_name)))
    }

    /// Check if an image is cached
    pub fn contains(&self, card_name: &str) -> bool {
        self.path(card_name).exists()
    }

    /// Get a cached image
    pub fn get(&self, card_name: &str) -> Option<Vec<u8>> {
        let path = self.path(card_name);
        match std::fs::read(&path) {
            Ok(bytes) => {
                log::debug!("Image cache hit for: {}", card_name);
                Some(bytes)
            }
            Err(_) => None,
        }
    }

    /// Store an image in the cache
    pub fn insert(&self, card_name: &str, bytes: &[u8]) {
        let path = self.path(card_name);
        if let Err(e) = std::fs::write(&path, bytes) {
            log::warn!("Failed to cache image for {}: {}", card_name, e);
        } else {
            log::debug!("Cached image for: {}", card_name);
        }
    }

    /// Get cached card metadata
    pub fn get_meta(&self, card_name: &str) -> Option<CardInfo> {
        let path = self.meta_path(card_name);
        match std::fs::read_to_string(&path) {
            Ok(json) => match serde_json::from_str(&json) {
                Ok(info) => Some(info),
                Err(e) => {
                    log::warn!("Failed to parse cached metadata for {}: {}", card_name, e);
                    None
                }
            },
            Err(_) => None,
        }
    }

    /// Store card metadata in the cache
    pub fn insert_meta(&self, card_name: &str, info: &CardInfo) {
        let path = self.meta_path(card_name);
        match serde_json::to_string(info) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    log::warn!("Failed to cache metadata for {}: {}", card_name, e);
                }
            }
            Err(e) => {
                log::warn!("Failed to serialize metadata for {}: {}", card_name, e);
            }
        }
    }
}

/// Fetch a card image, checking cache first.
/// Also caches card metadata alongside the image.
pub async fn fetch_image_cached(
    cache: &ImageCache,
    card_name: &str,
) -> Result<Vec<u8>, InventoryError> {
    // Check cache first
    if let Some(bytes) = cache.get(card_name) {
        return Ok(bytes);
    }

    // Fetch card data from Scryfall
    log::info!(
        "Image cache miss for '{}', fetching from Scryfall",
        card_name
    );
    let card = fetch_card_by_name(card_name).await?;

    // Cache metadata alongside image
    cache.insert_meta(card_name, &card.card_info());

    // Get image URL
    let image_url = card
        .image_url()
        .ok_or_else(|| InventoryError::NoImageAvailable(card_name.to_string()))?;

    // Fetch image bytes
    let bytes = fetch_image(image_url).await?;

    // Store in cache
    cache.insert(card_name, &bytes);

    Ok(bytes)
}

/// Fetch card info (metadata), checking cache first.
/// If not cached, fetches from Scryfall and caches both image and metadata.
pub async fn fetch_card_info_cached(
    cache: &ImageCache,
    card_name: &str,
) -> Result<CardInfo, InventoryError> {
    // Check metadata cache first
    if let Some(info) = cache.get_meta(card_name) {
        return Ok(info);
    }

    // Fetch from Scryfall
    log::info!(
        "Metadata cache miss for '{}', fetching from Scryfall",
        card_name
    );
    let card = fetch_card_by_name(card_name).await?;
    let info = card.card_info();

    // Cache metadata
    cache.insert_meta(card_name, &info);

    // Also cache image if not already cached
    if !cache.contains(card_name) {
        if let Some(image_url) = card.image_url() {
            match fetch_image(image_url).await {
                Ok(bytes) => cache.insert(card_name, &bytes),
                Err(e) => log::warn!("Failed to cache image for '{}': {}", card_name, e),
            }
        }
    }

    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(ImageCache::sanitize_filename("Black Lotus"), "black lotus");
        assert_eq!(
            ImageCache::sanitize_filename("Jace, the Mind Sculptor"),
            "jace, the mind sculptor"
        );
        assert_eq!(ImageCache::sanitize_filename("Fire // Ice"), "fire __ ice");
        assert_eq!(
            ImageCache::sanitize_filename("Who/What/When/Where/Why"),
            "who_what_when_where_why"
        );
    }

    #[test]
    fn test_filename_format() {
        assert_eq!(ImageCache::filename("Black Lotus"), "black lotus.jpg");
        assert_eq!(ImageCache::filename("Fire // Ice"), "fire __ ice.jpg");
    }

    #[test]
    fn test_insert_and_get() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let cache = ImageCache::new(temp_dir.path());
        let test_data = vec![0xFF, 0xD8, 0xFF]; // JPEG magic bytes

        assert!(!cache.contains("Black Lotus"));

        cache.insert("Black Lotus", &test_data);

        assert!(cache.contains("Black Lotus"));
        let retrieved = cache.get("Black Lotus");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), test_data);
    }

    #[test]
    fn test_metadata_cache() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let cache = ImageCache::new(temp_dir.path());

        // No metadata initially
        assert!(cache.get_meta("Lightning Bolt").is_none());

        let info = CardInfo {
            set_name: Some("Ravnica: Clue Edition".to_string()),
            type_line: Some("Instant".to_string()),
            mana_cost: Some("{R}".to_string()),
            rarity: Some("uncommon".to_string()),
            oracle_text: Some("Deals 3 damage.".to_string()),
            purchase_uris: None,
        };

        cache.insert_meta("Lightning Bolt", &info);

        let retrieved = cache.get_meta("Lightning Bolt");
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.set_name.as_deref(), Some("Ravnica: Clue Edition"));
        assert_eq!(retrieved.type_line.as_deref(), Some("Instant"));
        assert_eq!(retrieved.mana_cost.as_deref(), Some("{R}"));
        assert_eq!(retrieved.rarity.as_deref(), Some("uncommon"));
    }

    #[test]
    fn test_case_insensitive() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let cache = ImageCache::new(temp_dir.path());
        let test_data = vec![1, 2, 3, 4, 5];

        cache.insert("Black Lotus", &test_data);

        // Should work with different cases
        assert!(cache.get("black lotus").is_some());
        assert!(cache.get("BLACK LOTUS").is_some());
        assert!(cache.get("Black Lotus").is_some());
    }
}
