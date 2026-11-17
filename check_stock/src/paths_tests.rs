//! The desktop app must keep finding the files it has always used: a changed
//! path would silently start an empty inventory DB or a fresh ruleset. Each
//! test spells out the location the app used before `check_stock_core` was
//! split off, so these tests never touch the disk.

use super::*;

/// `<platform dir>/d2d_automations`, written out the way the app always built it.
fn legacy(base: Option<PathBuf>) -> PathBuf {
    base.unwrap_or_else(|| PathBuf::from("."))
        .join("d2d_automations")
}

#[test]
fn inventory_db_is_where_it_always_was() {
    assert_eq!(
        inventory_db().path(),
        legacy(dirs::data_dir()).join("inventory.db")
    );
}

#[test]
fn ruleset_is_where_it_always_was() {
    assert_eq!(
        ruleset_file(),
        legacy(dirs::config_dir()).join("repricing_ruleset.json")
    );
}

#[test]
fn roots_are_the_platform_folders() {
    assert_eq!(data_root(), legacy(dirs::data_dir()));
    assert_eq!(cache_root(), legacy(dirs::cache_dir()));
    assert_eq!(config_root(), legacy(dirs::config_dir()));
}

#[test]
fn caches_keep_their_folder_names() {
    // Together with `cache_root`, these give exactly the old cache locations.
    assert_eq!(ImageCache::DIR_NAME, "images");
    assert_eq!(CardTypeCache::DIR_NAME, "card_types");
    assert_eq!(DeckCache::DIR_NAME, "mtgjson_v1");
    assert_eq!(SalesCache::DIR_NAME, "precon_sales");
    assert_eq!(CardCache::FILE_NAME, "scryfall_cache.json");
}

#[test]
fn missing_platform_folder_falls_back_to_the_working_directory() {
    assert_eq!(app_dir(None), PathBuf::from("./d2d_automations"));
    assert_eq!(
        ruleset_file_in(&app_dir(None)),
        PathBuf::from("./d2d_automations/repricing_ruleset.json")
    );
}
