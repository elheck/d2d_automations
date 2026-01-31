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

    /// Get the cache directory path
    pub fn cache_dir(&self) -> &std::path::Path {
        &self.cache_dir
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_cache() -> (ImageCache, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let cache = ImageCache {
            cache_dir: temp_dir.path().to_path_buf(),
        };
        (cache, temp_dir)
    }

    #[test]
    fn test_filename_format() {
        assert_eq!(ImageCache::filename("LEA", "123"), "lea_123.jpg");
        assert_eq!(ImageCache::filename("Hou", "45"), "hou_45.jpg");
        assert_eq!(ImageCache::filename("abc", "1"), "abc_1.jpg");
    }

    #[test]
    fn test_filename_lowercase() {
        assert_eq!(ImageCache::filename("LEA", "123"), "lea_123.jpg");
        assert_eq!(ImageCache::filename("HOU", "456"), "hou_456.jpg");
    }

    #[test]
    fn test_path_construction() {
        let (cache, _temp_dir) = create_test_cache();
        let path = cache.path("lea", "123");

        assert!(path.ends_with("lea_123.jpg"));
    }

    #[test]
    fn test_get_nonexistent_returns_none() {
        let (cache, _temp_dir) = create_test_cache();
        assert!(cache.get("lea", "999").is_none());
    }

    #[test]
    fn test_contains_nonexistent_returns_false() {
        let (cache, _temp_dir) = create_test_cache();
        assert!(!cache.contains("lea", "999"));
    }

    #[test]
    fn test_insert_and_get() {
        let (cache, _temp_dir) = create_test_cache();
        let test_data = vec![0x89, 0x50, 0x4E, 0x47]; // PNG magic bytes

        cache.insert("lea", "123", &test_data);

        let retrieved = cache.get("lea", "123");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), test_data);
    }

    #[test]
    fn test_insert_and_contains() {
        let (cache, _temp_dir) = create_test_cache();
        let test_data = vec![0xFF, 0xD8, 0xFF]; // JPEG magic bytes

        assert!(!cache.contains("lea", "456"));

        cache.insert("lea", "456", &test_data);

        assert!(cache.contains("lea", "456"));
    }

    #[test]
    fn test_get_case_insensitive_set_code() {
        let (cache, _temp_dir) = create_test_cache();
        let test_data = vec![1, 2, 3, 4, 5];

        cache.insert("LEA", "123", &test_data);

        // Should find with different cases (key is lowercased)
        assert!(cache.get("lea", "123").is_some());
        assert!(cache.get("LEA", "123").is_some());
        assert!(cache.get("Lea", "123").is_some());
    }

    #[test]
    fn test_insert_overwrites_existing() {
        let (cache, _temp_dir) = create_test_cache();
        let data1 = vec![1, 2, 3];
        let data2 = vec![4, 5, 6, 7];

        cache.insert("lea", "100", &data1);
        cache.insert("lea", "100", &data2);

        let retrieved = cache.get("lea", "100").unwrap();
        assert_eq!(retrieved, data2);
    }

    #[test]
    fn test_multiple_images() {
        let (cache, _temp_dir) = create_test_cache();

        cache.insert("lea", "1", &[1, 1, 1]);
        cache.insert("lea", "2", &[2, 2, 2]);
        cache.insert("hou", "1", &[3, 3, 3]);

        assert_eq!(cache.get("lea", "1").unwrap(), vec![1, 1, 1]);
        assert_eq!(cache.get("lea", "2").unwrap(), vec![2, 2, 2]);
        assert_eq!(cache.get("hou", "1").unwrap(), vec![3, 3, 3]);
    }

    #[test]
    fn test_files_persist_on_disk() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().to_path_buf();

        // Create cache and insert data
        {
            let cache = ImageCache {
                cache_dir: cache_dir.clone(),
            };
            cache.insert("lea", "123", &[10, 20, 30]);
        }

        // Create new cache pointing to same directory
        {
            let cache = ImageCache {
                cache_dir: cache_dir.clone(),
            };
            let retrieved = cache.get("lea", "123");
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap(), vec![10, 20, 30]);
        }
    }

    #[test]
    fn test_empty_image_data() {
        let (cache, _temp_dir) = create_test_cache();
        let empty_data: Vec<u8> = vec![];

        cache.insert("lea", "empty", &empty_data);

        let retrieved = cache.get("lea", "empty");
        assert!(retrieved.is_some());
        assert!(retrieved.unwrap().is_empty());
    }

    #[test]
    fn test_large_image_data() {
        let (cache, _temp_dir) = create_test_cache();
        let large_data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();

        cache.insert("lea", "large", &large_data);

        let retrieved = cache.get("lea", "large");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), large_data);
    }
}
