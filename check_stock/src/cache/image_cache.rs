use crate::api::scryfall::fetch_image;
use crate::error::ApiResult;

/// Persistent cache for card images
/// Stores images as files in the cache directory
pub struct ImageCache {
    cache_dir: std::path::PathBuf,
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageCache {
    /// Create a new image cache
    pub fn new() -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("d2d_automations")
            .join("images");

        // Create directory if needed
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            log::warn!("Failed to create image cache directory: {}", e);
        }

        log::info!("Image cache directory: {:?}", cache_dir);
        Self { cache_dir }
    }

    /// Generate a filename from set code and collector number
    fn filename(set_code: &str, collector_number: &str) -> String {
        format!("{}_{}.jpg", set_code.to_lowercase(), collector_number)
    }

    /// Get the full path for a cached image
    fn path(&self, set_code: &str, collector_number: &str) -> std::path::PathBuf {
        self.cache_dir
            .join(Self::filename(set_code, collector_number))
    }

    /// Check if an image is cached
    #[allow(dead_code)]
    pub fn contains(&self, set_code: &str, collector_number: &str) -> bool {
        self.path(set_code, collector_number).exists()
    }

    /// Get a cached image
    pub fn get(&self, set_code: &str, collector_number: &str) -> Option<Vec<u8>> {
        let path = self.path(set_code, collector_number);
        match std::fs::read(&path) {
            Ok(bytes) => {
                log::info!("Image cache hit for {}/{}", set_code, collector_number);
                Some(bytes)
            }
            Err(_) => None,
        }
    }

    /// Store an image in the cache
    pub fn insert(&self, set_code: &str, collector_number: &str, bytes: &[u8]) {
        let path = self.path(set_code, collector_number);
        if let Err(e) = std::fs::write(&path, bytes) {
            log::warn!("Failed to cache image: {}", e);
        } else {
            log::debug!("Cached image for {}/{}", set_code, collector_number);
        }
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
