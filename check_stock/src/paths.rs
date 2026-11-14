//! Where the desktop app keeps its files: the platform data, cache and config
//! directories, each with a `d2d_automations` folder. `check_stock_core`
//! never picks a location itself; everything is passed in from here.
//!
//! The locations are the ones the app has always used, so existing databases,
//! caches and the saved repricing ruleset keep being found:
//!
//! | What | Linux |
//! |---|---|
//! | Inventory DB + its backups | `~/.local/share/d2d_automations/inventory.db` |
//! | Caches (images, card types, MTGJSON, sales, Scryfall) | `~/.cache/d2d_automations/` |
//! | Repricing ruleset | `~/.config/d2d_automations/repricing_ruleset.json` |

use crate::cache::{CardCache, CardTypeCache, DeckCache, ImageCache, SalesCache};
use crate::inventory_db::InventoryDb;
use crate::repricing::RULESET_FILE_NAME;
use std::path::{Path, PathBuf};

/// Folder name inside each platform directory.
pub const APP_DIR: &str = "d2d_automations";

/// `<base>/d2d_automations`, or `./d2d_automations` when the platform has no
/// such directory.
fn app_dir(base: Option<PathBuf>) -> PathBuf {
    base.unwrap_or_else(|| PathBuf::from(".")).join(APP_DIR)
}

/// Platform data folder of the app (Linux: `~/.local/share/d2d_automations`).
pub fn data_root() -> PathBuf {
    app_dir(dirs::data_dir())
}

/// Platform cache folder of the app (Linux: `~/.cache/d2d_automations`).
pub fn cache_root() -> PathBuf {
    app_dir(dirs::cache_dir())
}

/// Platform config folder of the app (Linux: `~/.config/d2d_automations`).
pub fn config_root() -> PathBuf {
    app_dir(dirs::config_dir())
}

/// The inventory database.
pub fn inventory_db() -> InventoryDb {
    InventoryDb::in_dir(&data_root())
}

/// The working repricing ruleset, saved between sessions.
pub fn ruleset_file() -> PathBuf {
    ruleset_file_in(&config_root())
}

fn ruleset_file_in(config_root: &Path) -> PathBuf {
    config_root.join(RULESET_FILE_NAME)
}

/// Card image cache.
pub fn image_cache() -> ImageCache {
    ImageCache::in_root(&cache_root())
}

/// Scryfall card cache (loaded from disk).
pub fn card_cache() -> CardCache {
    CardCache::load_in(&cache_root())
}

/// Card type-line cache for the bulk-lot type filter (loaded from disk).
pub fn card_type_cache() -> CardTypeCache {
    CardTypeCache::in_root(&cache_root())
}

/// MTGJSON decklist + sealed-index cache.
pub fn deck_cache() -> DeckCache {
    DeckCache::in_root(&cache_root())
}

/// Daily precon sales-stats cache.
pub fn sales_cache() -> SalesCache {
    SalesCache::in_root(&cache_root())
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
