use super::*;
use crate::inventory_db::InStockCard;

fn card(effective_date: &str, quantity: i64, price: f64) -> InStockCard {
    InStockCard {
        cardmarket_id: "1".to_string(),
        name: "Test".to_string(),
        set_code: "TST".to_string(),
        cn: "1".to_string(),
        condition: "NM".to_string(),
        language: "English".to_string(),
        is_foil: false,
        rarity: "common".to_string(),
        quantity,
        price,
        location: String::new(),
        effective_date: effective_date.to_string(),
    }
}

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 14).unwrap()
}

#[test]
fn always_returns_all_five_buckets() {
    let buckets = bucket_cards(&[], today());
    assert_eq!(buckets.len(), 5);
    assert!(buckets.iter().all(|b| b.listings == 0 && b.copies == 0));
    assert_eq!(buckets[0].label, "0–30 days");
    assert_eq!(buckets[4].label, "365+ days");
    assert_eq!(buckets[4].max_days, None);
}

#[test]
fn recent_card_lands_in_first_bucket() {
    // 10 days old.
    let buckets = bucket_cards(&[card("2026-07-04", 3, 2.0)], today());
    assert_eq!(buckets[0].listings, 1);
    assert_eq!(buckets[0].copies, 3);
    assert!((buckets[0].value - 6.0).abs() < 0.001);
}

#[test]
fn boundary_ages_fall_in_expected_buckets() {
    // Exactly 30 days → bucket 0; exactly 31 days → bucket 1.
    let b30 = bucket_cards(&[card("2026-06-14", 1, 1.0)], today());
    assert_eq!(b30[0].copies, 1);
    assert_eq!(b30[1].copies, 0);

    let b31 = bucket_cards(&[card("2026-06-13", 1, 1.0)], today());
    assert_eq!(b31[0].copies, 0);
    assert_eq!(b31[1].copies, 1);
}

#[test]
fn very_old_card_lands_in_open_ended_bucket() {
    let buckets = bucket_cards(&[card("2024-01-01", 2, 5.0)], today());
    assert_eq!(buckets[4].copies, 2);
    assert!((buckets[4].value - 10.0).abs() < 0.001);
}

#[test]
fn unparseable_date_treated_as_newest() {
    let buckets = bucket_cards(&[card("", 1, 1.0)], today());
    assert_eq!(buckets[0].copies, 1, "empty date should count as age 0");
}

#[test]
fn future_date_clamped_to_zero_age() {
    let buckets = bucket_cards(&[card("2027-01-01", 1, 1.0)], today());
    assert_eq!(buckets[0].copies, 1);
}

#[test]
fn multiple_cards_accumulate_across_buckets() {
    let cards = vec![
        card("2026-07-10", 2, 1.0), // ~4 days  -> bucket 0
        card("2026-05-01", 1, 3.0), // ~74 days -> bucket 1
        card("2023-01-01", 5, 4.0), // years    -> bucket 4
    ];
    let buckets = bucket_cards(&cards, today());
    assert_eq!(buckets[0].copies, 2);
    assert_eq!(buckets[1].copies, 1);
    assert_eq!(buckets[4].copies, 5);
    let total_value: f64 = buckets.iter().map(|b| b.value).sum();
    assert!((total_value - (2.0 + 3.0 + 20.0)).abs() < 0.001);
}
