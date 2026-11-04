//! Permanent cache of card type lines per printing (set code + collector
//! number), for the bulk lot builder's type filter.
//!
//! A printing's type line practically never changes, so found entries never
//! expire. Printings Scryfall could not resolve are remembered for
//! [`MISSING_RECHECK_DAYS`] so they are not looked up on every load.
//!
//! The file is user-writable and therefore untrusted: type lines are only
//! ever compared against card-type words, never used in paths or URLs, and
//! oversized entries are dropped on load.

use mtg_common::FileCache;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Bump when the file format changes; older files are ignored.
pub const TYPE_CACHE_SCHEMA: u32 = 1;
/// Days before a printing Scryfall did not know is looked up again.
pub const MISSING_RECHECK_DAYS: i64 = 30;
const TYPES_FILE: &str = "card_types.json";
/// Longer values cannot be real type lines.
const MAX_TYPE_LINE_CHARS: usize = 200;

#[derive(Serialize, Deserialize, Default)]
struct CachedTypes {
    schema: u32,
    /// `set/cn` → front-face type line.
    types: HashMap<String, String>,
    /// `set/cn` → day (`YYYY-MM-DD`) Scryfall last failed to resolve it.
    missing: HashMap<String, String>,
}

/// Type-line cache under `<cache>/d2d_automations/card_types/`.
pub struct CardTypeCache {
    files: FileCache,
    data: CachedTypes,
    dirty: bool,
}

impl Default for CardTypeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CardTypeCache {
    /// Loads the cache from the platform cache directory.
    pub fn new() -> Self {
        let dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("d2d_automations")
            .join("card_types");
        Self::with_dir(dir)
    }

    /// Loads a cache rooted at the given directory (used by tests).
    pub fn with_dir(dir: PathBuf) -> Self {
        let files = FileCache::new(dir);
        let data = files
            .read(TYPES_FILE)
            .and_then(
                |bytes| match serde_json::from_slice::<CachedTypes>(&bytes) {
                    Ok(c) if c.schema == TYPE_CACHE_SCHEMA => Some(c),
                    Ok(_) => None,
                    Err(e) => {
                        log::warn!("Ignoring unreadable card type cache: {e}");
                        None
                    }
                },
            )
            .map(|mut c| {
                c.types
                    .retain(|_, t| t.chars().count() <= MAX_TYPE_LINE_CHARS);
                c
            })
            .unwrap_or_else(|| CachedTypes {
                schema: TYPE_CACHE_SCHEMA,
                ..CachedTypes::default()
            });
        Self {
            files,
            data,
            dirty: false,
        }
    }

    /// Cache key of a printing: lowercase set code, `/`, collector number.
    pub fn key(set_code: &str, cn: &str) -> String {
        format!("{}/{}", set_code.trim().to_lowercase(), cn.trim())
    }

    /// The printing's front-face type line, if known.
    pub fn type_line(&self, set_code: &str, cn: &str) -> Option<&str> {
        self.data
            .types
            .get(&Self::key(set_code, cn))
            .map(String::as_str)
    }

    /// Whether looking the printing up again is pointless: its type is
    /// known, or Scryfall failed to resolve it less than
    /// [`MISSING_RECHECK_DAYS`] before `today`.
    pub fn is_settled(&self, set_code: &str, cn: &str, today: chrono::NaiveDate) -> bool {
        let key = Self::key(set_code, cn);
        self.data.types.contains_key(&key)
            || self.data.missing.get(&key).is_some_and(|day| {
                chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
                    .is_ok_and(|d| (today - d).num_days() < MISSING_RECHECK_DAYS)
            })
    }

    pub fn insert(&mut self, set_code: &str, cn: &str, type_line: &str) {
        let type_line = type_line.trim();
        if type_line.is_empty() || type_line.chars().count() > MAX_TYPE_LINE_CHARS {
            return;
        }
        let key = Self::key(set_code, cn);
        self.data.missing.remove(&key);
        self.data.types.insert(key, type_line.to_string());
        self.dirty = true;
    }

    /// Remembers that Scryfall could not resolve the printing on `today`.
    pub fn insert_missing(&mut self, set_code: &str, cn: &str, today: chrono::NaiveDate) {
        self.data.missing.insert(
            Self::key(set_code, cn),
            today.format("%Y-%m-%d").to_string(),
        );
        self.dirty = true;
    }

    /// Number of printings with a known type line.
    pub fn len(&self) -> usize {
        self.data.types.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.types.is_empty()
    }

    /// Writes the cache to disk if it changed (best-effort).
    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        match serde_json::to_vec(&self.data) {
            Ok(bytes) => {
                self.files.write(TYPES_FILE, &bytes);
                self.dirty = false;
            }
            Err(e) => log::warn!("Failed to serialize card type cache: {e}"),
        }
    }
}

#[cfg(test)]
#[path = "card_type_cache_tests.rs"]
mod tests;
