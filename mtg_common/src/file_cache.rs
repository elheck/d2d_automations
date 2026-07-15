//! Simple persistent byte cache backed by files in a single directory.
//!
//! Shared foundation for the image/metadata caches in the GUI and server
//! projects; callers decide the filename scheme.

use std::path::{Path, PathBuf};

/// Persistent cache storing blobs as individual files in one directory.
///
/// All operations are best-effort: write failures are logged, not returned,
/// because a broken cache should never take down the application.
pub struct FileCache {
    dir: PathBuf,
}

impl FileCache {
    /// Create a cache rooted at the given directory, creating it if needed.
    pub fn new(dir: PathBuf) -> Self {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("Failed to create cache directory {:?}: {}", dir, e);
        } else {
            log::info!("Cache directory: {:?}", dir);
        }
        Self { dir }
    }

    /// The cache directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Check whether an entry exists.
    pub fn contains(&self, filename: &str) -> bool {
        self.dir.join(filename).exists()
    }

    /// Read an entry, returning None if missing or unreadable.
    pub fn read(&self, filename: &str) -> Option<Vec<u8>> {
        std::fs::read(self.dir.join(filename)).ok()
    }

    /// Write an entry, logging (not returning) failures.
    pub fn write(&self, filename: &str, bytes: &[u8]) {
        let path = self.dir.join(filename);
        if let Err(e) = std::fs::write(&path, bytes) {
            log::warn!("Failed to write cache entry {:?}: {}", path, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_missing_returns_none() {
        let tmp = TempDir::new().unwrap();
        let cache = FileCache::new(tmp.path().to_path_buf());
        assert!(cache.read("missing.bin").is_none());
        assert!(!cache.contains("missing.bin"));
    }

    #[test]
    fn write_then_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let cache = FileCache::new(tmp.path().to_path_buf());

        cache.write("a.bin", &[1, 2, 3]);

        assert!(cache.contains("a.bin"));
        assert_eq!(cache.read("a.bin").unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn write_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        let cache = FileCache::new(tmp.path().to_path_buf());

        cache.write("a.bin", &[1, 2, 3]);
        cache.write("a.bin", &[4, 5]);

        assert_eq!(cache.read("a.bin").unwrap(), vec![4, 5]);
    }

    #[test]
    fn entries_persist_across_instances() {
        let tmp = TempDir::new().unwrap();
        FileCache::new(tmp.path().to_path_buf()).write("a.bin", &[9]);

        let reopened = FileCache::new(tmp.path().to_path_buf());
        assert_eq!(reopened.read("a.bin").unwrap(), vec![9]);
    }

    #[test]
    fn empty_entry_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let cache = FileCache::new(tmp.path().to_path_buf());

        cache.write("empty.bin", &[]);

        assert!(cache.read("empty.bin").unwrap().is_empty());
    }
}
