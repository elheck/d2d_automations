//! Daily cache of precon card sales stats.
//!
//! Sales stats are derived from weekly price snapshots on the inventory_sync
//! tracker — tens of thousands of indexed lookups across all precon cards.
//! The tracker only updates once a day, so the stats are cached for the
//! calendar day; later loads that day only fetch cards not seen yet. Prices
//! themselves are never cached.
//!
//! The file is user-writable and therefore untrusted; the counts are small
//! integers and flags, and [`crate::precon_ev::liquidity_weight`] clamps any
//! value.

use crate::precon_ev::{SalesMap, FOIL_LOOKBACK_DAYS, SALES_DAYS};
use mtg_common::FileCache;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Bump when the file format changes; older files are ignored.
pub const SALES_SCHEMA: u32 = 3;
const SALES_FILE: &str = "sales_stats.json";
const WINDOW: [u32; 2] = [SALES_DAYS, FOIL_LOOKBACK_DAYS];

#[derive(Serialize, Deserialize)]
struct CachedSales {
    schema: u32,
    /// Calendar day (`YYYY-MM-DD`) the stats were computed for.
    date: String,
    /// Sales window `[days, lookback days]`; a different window invalidates
    /// the file.
    window: [u32; 2],
    stats: SalesMap,
}

/// Sales-stats cache under `<cache>/d2d_automations/precon_sales/`.
pub struct SalesCache {
    files: FileCache,
}

impl Default for SalesCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SalesCache {
    /// Create the cache in the platform cache directory.
    pub fn new() -> Self {
        let dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("d2d_automations")
            .join("precon_sales");
        Self::with_dir(dir)
    }

    /// Create a cache rooted at the given directory (used by tests).
    pub fn with_dir(dir: PathBuf) -> Self {
        Self {
            files: FileCache::new(dir),
        }
    }

    /// The stats cached for `date`, or `None` if missing, from another day,
    /// schema or sales window, or unreadable.
    pub fn load(&self, date: &str) -> Option<SalesMap> {
        let bytes = self.files.read(SALES_FILE)?;
        match serde_json::from_slice::<CachedSales>(&bytes) {
            Ok(c) if c.schema == SALES_SCHEMA && c.date == date && c.window == WINDOW => {
                Some(c.stats)
            }
            Ok(_) => None,
            Err(e) => {
                log::warn!("Ignoring unreadable sales cache: {e}");
                None
            }
        }
    }

    /// Writes the stats for `date` (best-effort), replacing older days.
    pub fn store(&self, date: &str, stats: &SalesMap) {
        let cached = CachedSales {
            schema: SALES_SCHEMA,
            date: date.to_string(),
            window: WINDOW,
            stats: stats.clone(),
        };
        match serde_json::to_vec(&cached) {
            Ok(bytes) => self.files.write(SALES_FILE, &bytes),
            Err(e) => log::warn!("Failed to serialize sales cache: {e}"),
        }
    }
}

#[cfg(test)]
#[path = "sales_cache_tests.rs"]
mod tests;
