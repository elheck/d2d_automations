//! Persistent cache for MTGJSON precon decklists.
//!
//! Decklists never change once complete, so — like card images — they are
//! stored permanently, one compact JSON file per deck `fileName`. A deck is
//! only stored when every card has a Cardmarket id; incomplete decks are
//! fetched again on the next load, picking up MTGJSON's later fixes.
//!
//! The sealed-product index built from MTGJSON's 12 MB `SetList.json` is
//! cached next to the decks and rebuilt when [`needs_rebuild`] says so.
//! Prices and the precon catalog are never cached.
//!
//! The cache is user-writable and therefore untrusted: names are checked
//! against the deck-file whitelist before touching the filesystem, and every
//! deck read back is re-validated.

use crate::api::mtgjson::{is_safe_deck_file_name, PreconDeck, SealedIndex};
use mtg_common::FileCache;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

/// Bump when the cached index format changes; older files are rebuilt.
pub const INDEX_SCHEMA: u32 = 1;
/// The sealed index is rebuilt after this many seconds (7 days).
pub const INDEX_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;
/// Timestamps further in the future than this are treated as invalid.
const MAX_CLOCK_SKEW_SECS: i64 = 24 * 60 * 60;
const INDEX_FILE: &str = "sealed_index.json";

/// The cached sealed-product index plus the bookkeeping that decides when
/// to rebuild it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedIndex {
    pub schema: u32,
    pub fetched_at_unix: i64,
    /// Catalog product ids seen when the index was built. A catalog id not
    /// in this list is a new product, which triggers a rebuild.
    pub seen_catalog_ids: Vec<u64>,
    pub index: SealedIndex,
}

/// True when the cached index is missing, from another schema, older than
/// [`INDEX_MAX_AGE_SECS`], dated in the future, or has not seen every
/// current catalog product.
pub fn needs_rebuild(cached: Option<&CachedIndex>, catalog_ids: &[u64], now_unix: i64) -> bool {
    let Some(cached) = cached else {
        return true;
    };
    if cached.schema != INDEX_SCHEMA {
        return true;
    }
    let age = now_unix - cached.fetched_at_unix;
    if !(-MAX_CLOCK_SKEW_SECS..=INDEX_MAX_AGE_SECS).contains(&age) {
        return true;
    }
    let seen: HashSet<u64> = cached.seen_catalog_ids.iter().copied().collect();
    catalog_ids.iter().any(|id| !seen.contains(id))
}

/// Decklist + sealed-index cache under `<cache>/d2d_automations/mtgjson_v1/`.
pub struct DeckCache {
    index_files: FileCache,
    deck_files: FileCache,
}

impl Default for DeckCache {
    fn default() -> Self {
        Self::new()
    }
}

impl DeckCache {
    /// Create the cache in the platform cache directory.
    pub fn new() -> Self {
        let root = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("d2d_automations")
            .join("mtgjson_v1");
        Self::with_dir(root)
    }

    /// Create a cache rooted at the given directory (used by tests).
    pub fn with_dir(root: PathBuf) -> Self {
        Self {
            deck_files: FileCache::new(root.join("decks")),
            index_files: FileCache::new(root),
        }
    }

    /// Directory holding the deck files.
    pub fn deck_dir(&self) -> &std::path::Path {
        self.deck_files.dir()
    }

    fn deck_filename(file_name: &str) -> String {
        format!("{file_name}.json")
    }

    /// A cached deck, or `None` if missing, unsafe-named, corrupt or invalid.
    pub fn get(&self, file_name: &str) -> Option<PreconDeck> {
        if !is_safe_deck_file_name(file_name) {
            return None;
        }
        let bytes = self.deck_files.read(&Self::deck_filename(file_name))?;
        match serde_json::from_slice::<PreconDeck>(&bytes) {
            Ok(deck) if deck.validate().is_ok() => Some(deck),
            Ok(_) | Err(_) => {
                log::warn!("Ignoring invalid cached deck {file_name:?}");
                None
            }
        }
    }

    /// Stores a deck if its name is safe and it is complete and valid.
    /// Returns whether it was written.
    pub fn insert(&self, file_name: &str, deck: &PreconDeck) -> bool {
        if !is_safe_deck_file_name(file_name) || !deck.is_complete() || deck.validate().is_err() {
            return false;
        }
        match serde_json::to_vec(deck) {
            Ok(bytes) => {
                self.deck_files
                    .write(&Self::deck_filename(file_name), &bytes);
                true
            }
            Err(e) => {
                log::warn!("Failed to serialize deck {file_name:?}: {e}");
                false
            }
        }
    }

    /// The cached sealed index, or `None` if missing or unreadable.
    pub fn load_index(&self) -> Option<CachedIndex> {
        let bytes = self.index_files.read(INDEX_FILE)?;
        match serde_json::from_slice(&bytes) {
            Ok(index) => Some(index),
            Err(e) => {
                log::warn!("Ignoring unreadable sealed index cache: {e}");
                None
            }
        }
    }

    /// Writes the sealed index (best-effort).
    pub fn store_index(&self, index: &CachedIndex) {
        match serde_json::to_vec(index) {
            Ok(bytes) => self.index_files.write(INDEX_FILE, &bytes),
            Err(e) => log::warn!("Failed to serialize sealed index: {e}"),
        }
    }
}

#[cfg(test)]
#[path = "deck_cache_tests.rs"]
mod tests;
