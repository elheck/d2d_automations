//! Tests for cardmarket.

use super::*;

#[test]
fn price_guide_entry_deserializes_with_nulls() {
    let json = r#"{
        "idProduct": 12345,
        "idCategory": 1,
        "avg": 1.5,
        "low": 0.5,
        "trend": 1.2,
        "avg1": null,
        "avg7": null,
        "avg30": null,
        "avg-foil": null,
        "low-foil": null,
        "trend-foil": null,
        "avg1-foil": null,
        "avg7-foil": null,
        "avg30-foil": null
    }"#;

    let entry: PriceGuideEntry = serde_json::from_str(json).unwrap();
    assert_eq!(entry.id_product, 12345);
    assert_eq!(entry.avg, Some(1.5));
    assert_eq!(entry.avg1, None);
}

#[test]
fn price_guide_file_deserializes() {
    let json = r#"{
        "version": 1,
        "createdAt": "2026-03-01T10:00:00+0100",
        "priceGuides": [{
            "idProduct": 1,
            "idCategory": 1,
            "avg": 10.0,
            "low": 8.0,
            "trend": 9.5,
            "avg1": null,
            "avg7": null,
            "avg30": null,
            "avg-foil": null,
            "low-foil": null,
            "trend-foil": null,
            "avg1-foil": null,
            "avg7-foil": null,
            "avg30-foil": null
        }]
    }"#;

    let file: PriceGuideFile = serde_json::from_str(json).unwrap();
    assert_eq!(file.version, 1);
    assert_eq!(file.created_at, "2026-03-01T10:00:00+0100");
    assert_eq!(file.price_guides.len(), 1);
    assert_eq!(file.price_guides[0].id_product, 1);
}
