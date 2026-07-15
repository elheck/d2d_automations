//! Persistent cache for card images and metadata
//!
//! Stores images as JPG files and metadata as JSON files in the cache directory.
//! Uses Cardmarket product ID as the cache key for exact printing matches.

use crate::error::InventoryError;
use crate::scryfall::{fetch_card_by_cardmarket_id, fetch_image, CardInfo};
use mtg_common::FileCache;

/// Persistent cache for card images and metadata
pub struct ImageCache {
    files: FileCache,
}

impl ImageCache {
    /// Create a new image cache in the same directory as the database
    pub fn new(db_dir: &std::path::Path) -> Self {
        Self {
            files: FileCache::new(db_dir.join("card_images")),
        }
    }

    /// Filename for a cached image by product ID
    fn image_name(id_product: u64) -> String {
        format!("{}.jpg", id_product)
    }

    /// Filename for cached card metadata by product ID
    fn meta_name(id_product: u64) -> String {
        format!("{}.json", id_product)
    }

    /// Check if an image is cached
    pub fn contains_image(&self, id_product: u64) -> bool {
        self.files.contains(&Self::image_name(id_product))
    }

    /// Get a cached image
    pub fn get_image(&self, id_product: u64) -> Option<Vec<u8>> {
        let bytes = self.files.read(&Self::image_name(id_product))?;
        log::debug!("Image cache hit for product ID: {}", id_product);
        Some(bytes)
    }

    /// Store an image in the cache
    pub fn insert_image(&self, id_product: u64, bytes: &[u8]) {
        self.files.write(&Self::image_name(id_product), bytes);
        log::debug!("Cached image for product ID: {}", id_product);
    }

    /// Get cached card metadata
    pub fn get_meta(&self, id_product: u64) -> Option<CardInfo> {
        let json = self.files.read(&Self::meta_name(id_product))?;
        match serde_json::from_slice(&json) {
            Ok(info) => Some(info),
            Err(e) => {
                log::warn!(
                    "Failed to parse cached metadata for product {}: {}",
                    id_product,
                    e
                );
                None
            }
        }
    }

    /// Store card metadata in the cache
    pub fn insert_meta(&self, id_product: u64, info: &CardInfo) {
        match serde_json::to_vec(info) {
            Ok(json) => self.files.write(&Self::meta_name(id_product), &json),
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
    cache.insert_meta(id_product, &CardInfo::from(&card));

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
    let info = CardInfo::from(&card);

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
#[path = "image_cache_tests.rs"]
mod tests;
