//! Persistent cache for card images and metadata
//!
//! Stores images as JPG files and metadata as JSON files in the cache directory.
//! Uses Cardmarket product ID as the cache key for exact printing matches.

use crate::error::InventoryError;
use crate::scryfall::{CardInfo, Scryfall};
use mtg_common::FileCache;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a product Scryfall does not know (or has no image for) is not
/// asked about again.
pub const MISS_TTL: Duration = Duration::from_secs(30 * 24 * 3600);

fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

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

    /// Filename of the marker for a product Scryfall had nothing for.
    fn miss_name(id_product: u64) -> String {
        format!("{}.miss", id_product)
    }

    /// Whether Scryfall had nothing for this product within [`MISS_TTL`].
    pub fn is_recent_miss(&self, id_product: u64, now: SystemTime) -> bool {
        let Some(bytes) = self.files.read(&Self::miss_name(id_product)) else {
            return false;
        };
        let Some(at) = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|t| t.trim().parse::<u64>().ok())
        else {
            return false;
        };
        unix_secs(now).saturating_sub(at) < MISS_TTL.as_secs()
    }

    /// Remembers that Scryfall had nothing for this product.
    pub fn record_miss(&self, id_product: u64, now: SystemTime) {
        self.files.write(
            &Self::miss_name(id_product),
            unix_secs(now).to_string().as_bytes(),
        );
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

/// Whether an error means Scryfall has nothing for the product (worth
/// remembering), as opposed to a passing network problem.
pub fn is_permanent_miss(e: &InventoryError) -> bool {
    matches!(
        e,
        InventoryError::ScryfallNotFound(_) | InventoryError::NoImageAvailable(_)
    )
}

/// Fetch a card image by Cardmarket product ID, checking cache first.
/// Also caches card metadata alongside the image; remembers misses.
pub async fn fetch_image_cached(
    cache: &ImageCache,
    scryfall: &Scryfall,
    id_product: u64,
) -> Result<Vec<u8>, InventoryError> {
    if let Some(bytes) = cache.get_image(id_product) {
        return Ok(bytes);
    }
    log::info!("Image cache miss for product {id_product}, fetching from Scryfall");
    let result = async {
        let card = scryfall.card(id_product).await?;
        cache.insert_meta(id_product, &CardInfo::from(&card));
        let image_url = card
            .image_url()
            .ok_or_else(|| InventoryError::NoImageAvailable(format!("product:{id_product}")))?;
        let bytes = scryfall.image(image_url).await?;
        cache.insert_image(id_product, &bytes);
        Ok(bytes)
    }
    .await;
    if let Err(e) = &result {
        if is_permanent_miss(e) {
            cache.record_miss(id_product, SystemTime::now());
        }
    }
    result
}

/// Fetch card info (metadata) by Cardmarket product ID, checking cache first.
/// If not cached, fetches from Scryfall and caches both image and metadata.
pub async fn fetch_card_info_cached(
    cache: &ImageCache,
    scryfall: &Scryfall,
    id_product: u64,
) -> Result<CardInfo, InventoryError> {
    if let Some(info) = cache.get_meta(id_product) {
        return Ok(info);
    }
    log::info!("Metadata cache miss for product {id_product}, fetching from Scryfall");
    let card = match scryfall.card(id_product).await {
        Ok(card) => card,
        Err(e) => {
            if is_permanent_miss(&e) {
                cache.record_miss(id_product, SystemTime::now());
            }
            return Err(e);
        }
    };
    let info = CardInfo::from(&card);
    cache.insert_meta(id_product, &info);
    if !cache.contains_image(id_product) {
        if let Some(image_url) = card.image_url() {
            match scryfall.image(image_url).await {
                Ok(bytes) => cache.insert_image(id_product, &bytes),
                Err(e) => log::warn!("Failed to cache image for product {id_product}: {e}"),
            }
        }
    }
    Ok(info)
}

#[cfg(test)]
#[path = "image_cache_tests.rs"]
mod tests;
