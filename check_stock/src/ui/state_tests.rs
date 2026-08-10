use super::*;

fn report(upserted: usize, zeroed: usize) -> SyncReport {
    SyncReport {
        category: "Magic Single".to_string(),
        upserted,
        zeroed,
        error: None,
        forced: false,
    }
}

#[test]
fn summary_reports_upserted_and_zeroed_counts() {
    let s = report(12, 3).summary();
    assert!(s.contains("Magic Single"), "{s}");
    assert!(s.contains("12 variants updated"), "{s}");
    assert!(s.contains("3 zeroed out"), "{s}");
}

#[test]
fn summary_singularizes_a_single_variant() {
    assert!(report(1, 0).summary().contains("1 variant updated"));
}

#[test]
fn summary_marks_a_forced_sync() {
    let mut r = report(5, 2);
    r.forced = true;
    let s = r.summary();
    assert!(s.starts_with("Forced inventory sync"), "{s}");
}

#[test]
fn summary_reports_the_error_instead_of_counts_when_the_sync_failed() {
    let mut r = report(0, 0);
    r.error = Some("database is locked".to_string());
    let s = r.summary();
    assert!(s.contains("Inventory sync failed"), "{s}");
    assert!(s.contains("database is locked"), "{s}");
    assert!(!s.contains("zeroed out"), "{s}");
}

#[test]
fn mark_db_changed_bumps_the_generation() {
    let mut state = AppState::default();
    assert_eq!(state.db_generation, 0);
    state.mark_db_changed();
    assert_eq!(state.db_generation, 1);
}
