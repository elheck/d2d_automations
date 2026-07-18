//! Tests for inventory_sync.

use super::*;

#[test]
fn client_trims_trailing_slashes() {
    let client = InventorySyncClient::new("http://cardscanner.local:3000///");
    assert_eq!(client.base_url(), "http://cardscanner.local:3000");
}

#[test]
fn client_keeps_clean_url() {
    let client = InventorySyncClient::new("http://127.0.0.1:8080");
    assert_eq!(client.base_url(), "http://127.0.0.1:8080");
}

#[test]
fn history_query_formats_days() {
    assert_eq!(InventorySyncClient::history_query(Some(90)), "?days=90");
    assert_eq!(InventorySyncClient::history_query(None), "");
}

#[test]
fn api_response_ok_into_result() {
    let resp = ApiResponse::ok(42u32);
    assert_eq!(resp.into_result().unwrap(), 42);
}

#[test]
fn api_response_err_into_result() {
    let resp: ApiResponse<u32> = ApiResponse::err("too many IDs");
    let err = resp.into_result().unwrap_err();
    assert!(err.to_string().contains("too many IDs"));
}

#[test]
fn api_response_success_without_data_is_error() {
    let resp: ApiResponse<u32> = ApiResponse {
        success: true,
        data: None,
        error: None,
    };
    assert!(resp.into_result().is_err());
}

#[test]
fn latest_price_round_trips() {
    let price = LatestPrice {
        id_product: 12345,
        price_date: "2026-07-17".to_string(),
        avg: Some(1.5),
        low: Some(0.5),
        trend: Some(1.2),
        avg1: None,
        avg7: Some(1.3),
        avg30: Some(1.4),
        avg_foil: None,
        low_foil: None,
        trend_foil: Some(3.0),
        avg1_foil: None,
        avg7_foil: None,
        avg30_foil: None,
    };
    let json = serde_json::to_string(&price).unwrap();
    let back: LatestPrice = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id_product, 12345);
    assert_eq!(back.trend, Some(1.2));
    assert_eq!(back.avg1, None);
    assert_eq!(back.trend_foil, Some(3.0));
}

#[test]
fn api_response_envelope_deserializes_server_shape() {
    // Exactly what the server emits: no `error` key on success.
    let json = r#"{"success":true,"data":[{"id_product":7,"price_date":"2026-07-17",
        "avg":null,"low":null,"trend":2.5,"avg1":null,"avg7":null,"avg30":null,
        "avg_foil":null,"low_foil":null,"trend_foil":null,"avg1_foil":null,
        "avg7_foil":null,"avg30_foil":null}]}"#;
    let resp: ApiResponse<Vec<LatestPrice>> = serde_json::from_str(json).unwrap();
    let prices = resp.into_result().unwrap();
    assert_eq!(prices.len(), 1);
    assert_eq!(prices[0].id_product, 7);
    assert_eq!(prices[0].trend, Some(2.5));
}

#[test]
fn price_snapshot_request_serializes() {
    let req = PriceSnapshotRequest {
        ids: vec![1, 2],
        dates: vec!["2026-07-18".to_string(), "2026-06-18".to_string()],
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"ids\":[1,2]"));
    assert!(json.contains("2026-06-18"));
}

#[test]
fn price_data_deserializes() {
    let json = r#"{
        "product": {"id_product": 9, "name": "Black Lotus", "category_name": "Magic Single",
                    "id_expansion": 1, "expansion_name": "Alpha"},
        "history": [{"price_date": "2026-07-17", "avg": 1.0, "low": 0.5, "trend": 0.9,
                     "avg1": null, "avg7": null, "avg30": null, "avg_foil": null,
                     "low_foil": null, "trend_foil": null, "avg1_foil": null,
                     "avg7_foil": null, "avg30_foil": null}],
        "indicators": {"ema_7": [null], "ema_30": [null], "sma_20": [null], "rsi": [null],
                       "macd": [null], "macd_signal": [null], "macd_histogram": [null],
                       "bb_upper": [null], "bb_middle": [null], "bb_lower": [null],
                       "roc_7": [null], "roc_30": [null], "bb_percent_b": [null],
                       "bb_width": [null]},
        "cardmarket_signals": {"momentum_1_7": [null], "momentum_7_30": [null],
                               "floor_ratio": [0.55]}
    }"#;
    let data: PriceData = serde_json::from_str(json).unwrap();
    assert_eq!(data.product.name, "Black Lotus");
    assert_eq!(data.history.len(), 1);
    assert_eq!(data.cardmarket_signals.floor_ratio, vec![Some(0.55)]);
}

#[test]
fn price_field_serializes_by_variant_name() {
    // Saved node graphs in check_stock depend on these exact names.
    assert_eq!(
        serde_json::to_string(&PriceField::Trend).unwrap(),
        "\"Trend\""
    );
    assert_eq!(
        serde_json::to_string(&PriceField::Avg30).unwrap(),
        "\"Avg30\""
    );
    let back: PriceField = serde_json::from_str("\"Avg7\"").unwrap();
    assert_eq!(back, PriceField::Avg7);
}

#[test]
fn price_fields_picks_foil_and_nonfoil_columns() {
    let price = LatestPrice {
        id_product: 1,
        price_date: "2026-07-17".to_string(),
        avg: Some(1.0),
        low: Some(0.5),
        trend: Some(0.9),
        avg1: Some(1.1),
        avg7: Some(1.2),
        avg30: Some(1.3),
        avg_foil: Some(2.0),
        low_foil: None,
        trend_foil: Some(1.9),
        avg1_foil: Some(2.1),
        avg7_foil: Some(2.2),
        avg30_foil: Some(2.3),
    };
    assert_eq!(price.price_for(PriceField::Trend, false), Some(0.9));
    assert_eq!(price.price_for(PriceField::Trend, true), Some(1.9));
    assert_eq!(price.price_for(PriceField::Low, true), None);
    assert_eq!(price.price_for(PriceField::Avg30, false), Some(1.3));
}
