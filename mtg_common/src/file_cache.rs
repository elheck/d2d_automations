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
#[path = "file_cache_tests.rs"]
mod tests;
