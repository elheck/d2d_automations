//! Persistent cache for card images and metadata
//!
//! Stores images as JPG files and metadata as JSON files in the cache directory.
//! Uses Cardmarket product ID as the cache key for exact printing matches.

use crate::error::InventoryError;
use crate::scryfall::{fetch_card_by_cardmarket_id, fetch_image, CardInfo};
use std::path::PathBuf;

/// Persistent cache for card images and metadata
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

    /// Get the full path for a cached image by product ID
    fn image_path(&self, id_product: u64) -> PathBuf {
        self.cache_dir.join(format!("{}.jpg", id_product))
    }

    /// Get the full path for cached card metadata by product ID
    fn meta_path(&self, id_product: u64) -> PathBuf {
        self.cache_dir.join(format!("{}.json", id_product))
    }

    /// Check if an image is cached
    pub fn contains_image(&self, id_product: u64) -> bool {
        self.image_path(id_product).exists()
    }

    /// Get a cached image
    pub fn get_image(&self, id_product: u64) -> Option<Vec<u8>> {
        let path = self.image_path(id_product);
        match std::fs::read(&path) {
            Ok(bytes) => {
                log::debug!("Image cache hit for product ID: {}", id_product);
                Some(bytes)
            }
            Err(_) => None,
        }
    }

    /// Store an image in the cache
    pub fn insert_image(&self, id_product: u64, bytes: &[u8]) {
        let path = self.image_path(id_product);
        if let Err(e) = std::fs::write(&path, bytes) {
            log::warn!("Failed to cache image for product {}: {}", id_product, e);
        } else {
            log::debug!("Cached image for product ID: {}", id_product);
        }
    }

    /// Get cached card metadata
    pub fn get_meta(&self, id_product: u64) -> Option<CardInfo> {
        let path = self.meta_path(id_product);
        match std::fs::read_to_string(&path) {
            Ok(json) => match serde_json::from_str(&json) {
                Ok(info) => Some(info),
                Err(e) => {
                    log::warn!(
                        "Failed to parse cached metadata for product {}: {}",
                        id_product,
                        e
                    );
                    None
                }
            },
            Err(_) => None,
        }
    }

    /// Store card metadata in the cache
    pub fn insert_meta(&self, id_product: u64, info: &CardInfo) {
        let path = self.meta_path(id_product);
        match serde_json::to_string(info) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    log::warn!("Failed to cache metadata for product {}: {}", id_product, e);
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to serialize metadata for product {}: {}",
                    id_product,
                    e
                );
            }
        }
    }
}

/// Fetch a card image by Cardmarket product ID, checking cache first.
/// Also caches card metadata alongside the image.
pub async fn fetch_image_cached(
    cache: &ImageCache,
    id_product: u64,
) -> Result<Vec<u8>, InventoryError> {
    // Check cache first
    if let Some(bytes) = cache.get_image(id_product) {
        return Ok(bytes);
    }

    // Fetch card data from Scryfall using Cardmarket ID
    log::info!(
        "Image cache miss for product {}, fetching from Scryfall",
        id_product
    );
    let card = fetch_card_by_cardmarket_id(id_product).await?;

    // Cache metadata alongside image
    cache.insert_meta(id_product, &card.card_info());

    // Get image URL
    let image_url = card
        .image_url()
        .ok_or_else(|| InventoryError::NoImageAvailable(format!("product:{}", id_product)))?;

    // Fetch image bytes
    let bytes = fetch_image(image_url).await?;

    // Store in cache
    cache.insert_image(id_product, &bytes);

    Ok(bytes)
}

/// Fetch card info (metadata) by Cardmarket product ID, checking cache first.
/// If not cached, fetches from Scryfall and caches both image and metadata.
pub async fn fetch_card_info_cached(
    cache: &ImageCache,
    id_product: u64,
) -> Result<CardInfo, InventoryError> {
    // Check metadata cache first
    if let Some(info) = cache.get_meta(id_product) {
        return Ok(info);
    }

    // Fetch from Scryfall using Cardmarket ID
    log::info!(
        "Metadata cache miss for product {}, fetching from Scryfall",
        id_product
    );
    let card = fetch_card_by_cardmarket_id(id_product).await?;
    let info = card.card_info();

    // Cache metadata
    cache.insert_meta(id_product, &info);

    // Also cache image if not already cached
    if !cache.contains_image(id_product) {
        if let Some(image_url) = card.image_url() {
            match fetch_image(image_url).await {
                Ok(bytes) => cache.insert_image(id_product, &bytes),
                Err(e) => log::warn!("Failed to cache image for product {}: {}", id_product, e),
            }
        }
    }

    Ok(info)
}

#[cfg(test)]
mod tests {
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
}
