//! Dead-stock aging — pure bucketing of in-stock cards by how long they have
//! been listed.
//!
//! "Age" is measured from a card's [`InStockCard::effective_date`] (its
//! `listed_at`, or `first_synced_at` when the listing date is unknown) to a
//! caller-supplied reference date. This module is pure and free of any database
//! or wall-clock access so it can be tested deterministically.

use crate::inventory_db::{AgingBucket, InStockCard};
use chrono::NaiveDate;

/// Bucket definitions: `(label, min_days_inclusive, max_days_inclusive)`.
/// The final bucket is open-ended (`None`).
const BUCKETS: [(&str, i64, Option<i64>); 5] = [
    ("0–30 days", 0, Some(30)),
    ("31–90 days", 31, Some(90)),
    ("91–180 days", 91, Some(180)),
    ("181–365 days", 181, Some(365)),
    ("365+ days", 366, None),
];

/// Age of a card in whole days relative to `today`.
///
/// Cards whose `effective_date` cannot be parsed are treated as age `0` (newest),
/// so they never falsely inflate the dead-stock buckets. Future-dated listings
/// are clamped to `0` as well.
pub fn age_days(card: &InStockCard, today: NaiveDate) -> i64 {
    match NaiveDate::parse_from_str(&card.effective_date, "%Y-%m-%d") {
        Ok(listed) => (today - listed).num_days().max(0),
        Err(_) => 0,
    }
}

/// Returns which bucket index an age falls into. Ages below the first bucket's
/// minimum (only possible if the table changes) fold into bucket 0.
fn bucket_index(age: i64) -> usize {
    for (i, (_, min, max)) in BUCKETS.iter().enumerate() {
        let in_range = age >= *min && max.map(|hi| age <= hi).unwrap_or(true);
        if in_range {
            return i;
        }
    }
    // Open-ended last bucket guarantees a match for any age >= 0.
    BUCKETS.len() - 1
}

/// Buckets `cards` by listing age relative to `today`.
///
/// Always returns all [`BUCKETS`] in order (including empty ones) so callers can
/// render a stable table. `value` accumulates capital tied up (price × quantity).
pub fn bucket_cards(cards: &[InStockCard], today: NaiveDate) -> Vec<AgingBucket> {
    let mut buckets: Vec<AgingBucket> = BUCKETS
        .iter()
        .map(|(label, min, max)| AgingBucket {
            label,
            min_days: *min,
            max_days: *max,
            listings: 0,
            copies: 0,
            value: 0.0,
        })
        .collect();

    for card in cards {
        let idx = bucket_index(age_days(card, today));
        let b = &mut buckets[idx];
        b.listings += 1;
        b.copies += card.quantity;
        b.value += card.price * card.quantity as f64;
    }

    buckets
}

#[path = "aging_tests.rs"]
#[cfg(test)]
mod tests;
