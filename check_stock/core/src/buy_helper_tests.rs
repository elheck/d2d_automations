use super::*;
use crate::models::Card;

/// Build a test card with a given rarity, price and quantity.
fn card(rarity: &str, price: &str, qty: &str) -> Card {
    Card {
        rarity: rarity.into(),
        price: price.into(),
        quantity: qty.into(),
        ..Card::test_default()
    }
}

// ── classify ──────────────────────────────────────────────────────────────

#[test]
fn classify_rare_is_single_by_default() {
    let p = BuyParams::default();
    assert_eq!(classify(&card("Rare", "0.10", "1"), &p), CardClass::Single);
}

#[test]
fn classify_mythic_is_single_by_default() {
    let p = BuyParams::default();
    assert_eq!(
        classify(&card("Mythic", "0.05", "1"), &p),
        CardClass::Single
    );
}

#[test]
fn classify_common_below_threshold_is_bulk() {
    let p = BuyParams::default();
    assert_eq!(classify(&card("Common", "0.10", "1"), &p), CardClass::Bulk);
}

#[test]
fn classify_common_at_threshold_is_single() {
    // 0.50 default threshold is inclusive.
    let p = BuyParams::default();
    assert_eq!(
        classify(&card("Common", "0.50", "1"), &p),
        CardClass::Single
    );
}

#[test]
fn classify_price_threshold_disabled_ignores_price() {
    let p = BuyParams {
        min_single_price: None,
        ..BuyParams::default()
    };
    // High-value common, but price rule disabled and common not enabled -> bulk.
    assert_eq!(classify(&card("Common", "5.00", "1"), &p), CardClass::Bulk);
}

#[test]
fn classify_rarity_case_insensitive() {
    let p = BuyParams::default();
    assert_eq!(classify(&card("rare", "0.01", "1"), &p), CardClass::Single);
    assert_eq!(
        classify(&card("MYTHIC", "0.01", "1"), &p),
        CardClass::Single
    );
}

#[test]
fn classify_unknown_rarity_uses_price_only() {
    let p = BuyParams::default();
    assert_eq!(classify(&card("Land", "0.10", "1"), &p), CardClass::Bulk);
    assert_eq!(classify(&card("Land", "0.90", "1"), &p), CardClass::Single);
}

#[test]
fn classify_respects_enabled_rarities() {
    let p = BuyParams {
        single_common: true,
        single_uncommon: true,
        single_rare: false,
        single_mythic: false,
        min_single_price: None,
        ..BuyParams::default()
    };
    assert_eq!(
        classify(&card("Common", "0.01", "1"), &p),
        CardClass::Single
    );
    assert_eq!(
        classify(&card("Uncommon", "0.01", "1"), &p),
        CardClass::Single
    );
    assert_eq!(classify(&card("Rare", "0.01", "1"), &p), CardClass::Bulk);
    assert_eq!(classify(&card("Mythic", "0.01", "1"), &p), CardClass::Bulk);
}

// ── card_qty ──────────────────────────────────────────────────────────────

#[test]
fn card_qty_parses_and_defaults() {
    assert_eq!(card_qty(&card("Rare", "1.0", "3")), 3);
    assert_eq!(card_qty(&card("Rare", "1.0", "not_a_number")), 1);
    assert_eq!(card_qty(&card("Rare", "1.0", "-5")), 0);
}

// ── compute_summary ─────────────────────────────────────────────────────────

#[test]
fn summary_splits_singles_and_bulk() {
    let cards = vec![
        card("Rare", "1.00", "1"),   // single
        card("Mythic", "2.00", "1"), // single
        card("Common", "0.05", "1"), // bulk
        card("Common", "0.05", "1"), // bulk
    ];
    let s = compute_summary(&cards, &BuyParams::default());
    assert_eq!(s.single_rows, 2);
    assert_eq!(s.single_cards, 2);
    assert_eq!(s.bulk_rows, 2);
    assert_eq!(s.bulk_cards, 2);
    assert!((s.single_market_value - 3.00).abs() < 1e-9);
}

#[test]
fn summary_single_offer_is_percentage_of_value() {
    let cards = vec![card("Rare", "10.00", "1")];
    let p = BuyParams {
        single_buy_percent: 60.0,
        ..BuyParams::default()
    };
    let s = compute_summary(&cards, &p);
    assert!((s.single_offer - 6.00).abs() < 1e-9);
}

#[test]
fn summary_bulk_offer_is_proportional_rate() {
    // 500 bulk cards at 10 EUR / 1000 -> 5.00
    let cards = vec![card("Common", "0.01", "500")];
    let p = BuyParams {
        bulk_rate: 10.0,
        bulk_batch: 1000,
        ..BuyParams::default()
    };
    let s = compute_summary(&cards, &p);
    assert_eq!(s.bulk_cards, 500);
    assert!((s.bulk_offer - 5.00).abs() < 1e-9);
}

#[test]
fn summary_quantity_multiplies_value_and_count() {
    let cards = vec![card("Rare", "2.00", "3")];
    let s = compute_summary(&cards, &BuyParams::default());
    assert_eq!(s.single_cards, 3);
    assert!((s.single_market_value - 6.00).abs() < 1e-9);
}

#[test]
fn summary_total_is_singles_plus_bulk() {
    let cards = vec![
        card("Rare", "10.00", "1"),     // single: offer 6.00 @60%
        card("Common", "0.01", "1000"), // bulk: offer 10.00
    ];
    let s = compute_summary(&cards, &BuyParams::default());
    assert!((s.single_offer - 6.00).abs() < 1e-9);
    assert!((s.bulk_offer - 10.00).abs() < 1e-9);
    assert!((s.total_offer - 16.00).abs() < 1e-9);
}

#[test]
fn summary_zero_batch_yields_no_bulk_offer() {
    let cards = vec![card("Common", "0.01", "1000")];
    let p = BuyParams {
        bulk_batch: 0,
        ..BuyParams::default()
    };
    let s = compute_summary(&cards, &p);
    assert_eq!(s.bulk_offer, 0.0);
}

#[test]
fn summary_empty_input_is_all_zero() {
    let s = compute_summary(&[], &BuyParams::default());
    assert_eq!(s, BuySummary::default());
}

// ── export_csv ──────────────────────────────────────────────────────────────

#[test]
fn export_has_header_and_rows() {
    let cards = vec![card("Rare", "1.00", "1")];
    let csv = export_csv(&cards, &BuyParams::default()).unwrap();
    let mut lines = csv.lines();
    assert_eq!(
        lines.next().unwrap(),
        "name,set,setCode,cn,condition,language,isFoil,rarity,quantity,unitPrice,marketValue,class,offer"
    );
    // First data row is the single card.
    assert!(lines.next().unwrap().contains("single"));
}

#[test]
fn export_offer_column_sums_to_total() {
    let cards = vec![card("Rare", "10.00", "1"), card("Common", "0.01", "1000")];
    let params = BuyParams::default();
    let summary = compute_summary(&cards, &params);
    let csv = export_csv(&cards, &params).unwrap();

    // Sum the per-card offer column (skip header, stop at the blank spacer that
    // precedes the summary rows — its trailing field is empty and won't parse).
    let mut sum = 0.0;
    for line in csv.lines().skip(1) {
        match line.rsplit(',').next().unwrap().parse::<f64>() {
            Ok(offer) => sum += offer,
            Err(_) => break,
        }
    }
    assert!((sum - summary.total_offer).abs() < 0.01);
}

#[test]
fn export_escapes_names_with_commas() {
    let mut c = card("Rare", "1.00", "1");
    c.name = "Quicksilver, Speedster".into();
    let csv = export_csv(&[c], &BuyParams::default()).unwrap();
    assert!(csv.contains("\"Quicksilver, Speedster\""));
}

#[test]
fn export_contains_summary_rows() {
    let cards = vec![card("Rare", "1.00", "1")];
    let csv = export_csv(&cards, &BuyParams::default()).unwrap();
    assert!(csv.contains("=== SINGLES ==="));
    assert!(csv.contains("=== BULK ==="));
    assert!(csv.contains("=== TOTAL ==="));
}
