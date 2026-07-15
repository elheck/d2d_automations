use crate::api::scryfall::fetch_image;
use crate::error::ApiResult;
use mtg_common::FileCache;

/// Persistent cache for card images, keyed by set code + collector number.
/// Stores images as files in the cache directory.
pub struct ImageCache {
    files: FileCache,
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageCache {
    /// Create a new image cache in the platform cache directory
    pub fn new() -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("d2d_automations")
            .join("images");
        Self::with_dir(cache_dir)
    }

    /// Create an image cache rooted at the given directory (used by tests)
    pub fn with_dir(cache_dir: std::path::PathBuf) -> Self {
        Self {
            files: FileCache::new(cache_dir),
        }
    }

    /// Get the cache directory path
    pub fn cache_dir(&self) -> &std::path::Path {
        self.files.dir()
    }

    /// Generate a filename from set code and collector number
    fn filename(set_code: &str, collector_number: &str) -> String {
        format!("{}_{}.jpg", set_code.to_lowercase(), collector_number)
    }

    /// Check if an image is cached
    pub fn contains(&self, set_code: &str, collector_number: &str) -> bool {
        self.files
            .contains(&Self::filename(set_code, collector_number))
    }

    /// Get a cached image
    pub fn get(&self, set_code: &str, collector_number: &str) -> Option<Vec<u8>> {
        let bytes = self
            .files
            .read(&Self::filename(set_code, collector_number))?;
        log::info!("Image cache hit for {}/{}", set_code, collector_number);
        Some(bytes)
    }

    /// Store an image in the cache
    pub fn insert(&self, set_code: &str, collector_number: &str, bytes: &[u8]) {
        self.files
            .write(&Self::filename(set_code, collector_number), bytes);
        log::debug!("Cached image for {}/{}", set_code, collector_number);
    }
}

/// Fetch an image, checking cache first
pub fn fetch_image_cached(
    cache: &ImageCache,
    set_code: &str,
    collector_number: &str,
    url: &str,
) -> ApiResult<Vec<u8>> {
    // Check cache first
    if let Some(bytes) = cache.get(set_code, collector_number) {
        return Ok(bytes);
    }

    // Fetch from URL
    log::info!(
        "Image cache miss for {}/{}, fetching from Scryfall",
        set_code,
        collector_number
    );
    let bytes = fetch_image(url)?;

    // Store in cache
    cache.insert(set_code, collector_number, &bytes);

    Ok(bytes)
}

#[cfg(test)]
#[path = "image_cache_tests.rs"]
mod tests;
