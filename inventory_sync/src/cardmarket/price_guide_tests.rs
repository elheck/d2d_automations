//! Tests for price_guide.

use super::*;

#[test]
fn price_guide_from_entries() {
    let entries = vec![
        make_test_price_entry(1, Some(10.0)),
        make_test_price_entry(2, Some(20.0)),
    ];
    let guide = PriceGuide::from_entries(entries, "2026-02-01T10:00:00+0100");

    assert_eq!(guide.len(), 2);
    assert_eq!(guide.created_at(), "2026-02-01T10:00:00+0100");
    assert_eq!(guide.get(1).unwrap().trend, Some(10.0));
    assert_eq!(guide.get(2).unwrap().trend, Some(20.0));
    assert!(guide.get(999).is_none());
}
