use super::*;
use crate::precon_ev::SalesStats;

fn stats() -> SalesMap {
    SalesMap::from([
        (
            1,
            SalesStats {
                nonfoil_days: 3,
                foil_days: 0,
                observed_days: 11,
                nonfoil_recent: true,
                foil_recent: false,
            },
        ),
        (2, SalesStats::default()),
    ])
}

#[test]
fn round_trip_for_the_same_day() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = SalesCache::with_dir(tmp.path().to_path_buf());
    assert!(cache.load("2026-10-17").is_none());
    cache.store("2026-10-17", &stats());
    assert_eq!(cache.load("2026-10-17"), Some(stats()));
}

#[test]
fn other_days_are_ignored_and_replaced() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = SalesCache::with_dir(tmp.path().to_path_buf());
    cache.store("2026-10-16", &stats());
    assert!(cache.load("2026-10-17").is_none());
    cache.store("2026-10-17", &SalesMap::new());
    assert_eq!(cache.load("2026-10-17"), Some(SalesMap::new()));
    assert!(cache.load("2026-10-16").is_none());
}

#[test]
fn corrupt_schema_or_window_mismatch_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = SalesCache::with_dir(tmp.path().to_path_buf());
    std::fs::write(tmp.path().join("sales_stats.json"), b"{not json").unwrap();
    assert!(cache.load("2026-10-17").is_none());

    for (schema, window) in [
        (SALES_SCHEMA + 1, WINDOW),
        (SALES_SCHEMA, [SALES_DAYS + 1, FOIL_LOOKBACK_DAYS]),
    ] {
        let file = serde_json::json!({
            "schema": schema, "date": "2026-10-17", "window": window, "stats": {}
        });
        std::fs::write(tmp.path().join("sales_stats.json"), file.to_string()).unwrap();
        assert!(cache.load("2026-10-17").is_none());
    }
}
