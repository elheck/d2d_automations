//! Cardmarket price guide — shared implementation lives in `mtg_common`.
//!
//! `PriceGuide::fetch` returns `MtgError`, which converts into this crate's
//! `InventoryError` via `From` at call sites.

pub use mtg_common::cardmarket::{PriceGuide, PriceGuideEntry};

/// Create a test price entry with default values
#[cfg(test)]
pub fn make_test_price_entry(id_product: u64, trend: Option<f64>) -> PriceGuideEntry {
    PriceGuideEntry {
        id_product,
        id_category: 1,
        avg: trend,
        low: trend.map(|t| t * 0.8),
        trend,
        avg1: None,
        avg7: None,
        avg30: None,
        avg_foil: None,
        low_foil: None,
        trend_foil: None,
        avg1_foil: None,
        avg7_foil: None,
        avg30_foil: None,
    }
}

#[cfg(test)]
mod tests {
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
}
