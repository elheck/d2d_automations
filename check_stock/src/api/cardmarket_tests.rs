//! Tests for the Cardmarket price guide client.

use std::io::Write;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::cardmarket::PriceGuide;
use crate::error::ApiError;

/// Creates a valid price guide JSON string with the given entries.
fn price_guide_json(entries: &[(u64, f64, f64)]) -> String {
    let guides: Vec<serde_json::Value> = entries
        .iter()
        .map(|(id, avg, trend)| {
            serde_json::json!({
                "idProduct": id,
                "idCategory": 1,
                "avg": avg,
                "low": avg * 0.8,
                "trend": trend,
                "avg1": avg,
                "avg7": avg,
                "avg30": avg,
                "avg-foil": null,
                "low-foil": null,
                "trend-foil": null,
                "avg1-foil": null,
                "avg7-foil": null,
                "avg30-foil": null
            })
        })
        .collect();

    serde_json::json!({
        "version": 1,
        "createdAt": "2025-01-15T12:00:00Z",
        "priceGuides": guides
    })
    .to_string()
}

// ── PriceGuide::load ─────────────────────────────────────────────────

#[test]
fn load_from_file_success() {
    let json = price_guide_json(&[(100, 10.50, 11.00), (200, 5.25, 5.50)]);

    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "{json}").unwrap();

    let guide = PriceGuide::load(tmp.path().to_str().unwrap()).unwrap();
    assert_eq!(guide.len(), 2);
    assert!(!guide.is_empty());
}

#[test]
fn load_from_file_not_found() {
    let result = PriceGuide::load("/nonexistent/path/price_guide.json");
    assert!(result.is_err());
    match result.unwrap_err() {
        ApiError::Io(_) => {} // Expected
        other => panic!("Expected ApiError::Io, got: {other:?}"),
    }
}

#[test]
fn load_from_file_malformed_json() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "{{ not valid json").unwrap();

    let result = PriceGuide::load(tmp.path().to_str().unwrap());
    assert!(result.is_err());
    match result.unwrap_err() {
        ApiError::Parse(_) => {} // Expected
        other => panic!("Expected ApiError::Parse, got: {other:?}"),
    }
}

#[test]
fn load_creates_correct_hashmap() {
    let json = price_guide_json(&[(111, 10.0, 11.0), (222, 20.0, 21.0), (333, 30.0, 31.0)]);

    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "{json}").unwrap();

    let guide = PriceGuide::load(tmp.path().to_str().unwrap()).unwrap();

    // Indexed by id_product
    assert!(guide.get(111).is_some());
    assert!(guide.get(222).is_some());
    assert!(guide.get(333).is_some());
    assert!(guide.get(999).is_none());

    let entry = guide.get(111).unwrap();
    assert!((entry.avg.unwrap() - 10.0).abs() < 0.001);
    assert!((entry.trend.unwrap() - 11.0).abs() < 0.001);
}

// ── PriceGuide::fetch_from ───────────────────────────────────────────

#[tokio::test]
async fn fetch_from_success() {
    let mock_server = MockServer::start().await;

    let json = price_guide_json(&[(100, 10.0, 11.0)]);

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(json))
        .mount(&mock_server)
        .await;

    let url = mock_server.uri();
    let result = tokio::task::spawn_blocking(move || PriceGuide::fetch_from(&url))
        .await
        .unwrap();

    let guide = result.unwrap();
    assert_eq!(guide.len(), 1);
    assert!(guide.get(100).is_some());
}

#[tokio::test]
async fn fetch_from_404() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let url = mock_server.uri();
    let result = tokio::task::spawn_blocking(move || PriceGuide::fetch_from(&url))
        .await
        .unwrap();

    match result {
        Err(ApiError::HttpStatus(status)) => {
            assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
        }
        other => panic!("Expected ApiError::HttpStatus(404), got: {other:?}"),
    }
}

#[tokio::test]
async fn fetch_from_500() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    let url = mock_server.uri();
    let result = tokio::task::spawn_blocking(move || PriceGuide::fetch_from(&url))
        .await
        .unwrap();

    match result {
        Err(ApiError::HttpStatus(status)) => {
            assert_eq!(status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
        }
        other => panic!("Expected ApiError::HttpStatus(500), got: {other:?}"),
    }
}

// ── PriceGuide::get / len / is_empty ─────────────────────────────────

#[test]
fn get_existing_entry() {
    let json = price_guide_json(&[(42, 15.75, 16.00)]);
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "{json}").unwrap();

    let guide = PriceGuide::load(tmp.path().to_str().unwrap()).unwrap();
    let entry = guide.get(42).unwrap();
    assert_eq!(entry.id_product, 42);
    assert!((entry.avg.unwrap() - 15.75).abs() < 0.001);
}

#[test]
fn get_nonexistent_entry() {
    let json = price_guide_json(&[(1, 1.0, 1.0)]);
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "{json}").unwrap();

    let guide = PriceGuide::load(tmp.path().to_str().unwrap()).unwrap();
    assert!(guide.get(99999).is_none());
}

#[test]
fn len_and_is_empty() {
    // Non-empty guide
    let json = price_guide_json(&[(1, 1.0, 1.0), (2, 2.0, 2.0)]);
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "{json}").unwrap();

    let guide = PriceGuide::load(tmp.path().to_str().unwrap()).unwrap();
    assert_eq!(guide.len(), 2);
    assert!(!guide.is_empty());

    // Empty guide
    let empty_json = price_guide_json(&[]);
    let mut tmp2 = tempfile::NamedTempFile::new().unwrap();
    write!(tmp2, "{empty_json}").unwrap();

    let empty_guide = PriceGuide::load(tmp2.path().to_str().unwrap()).unwrap();
    assert_eq!(empty_guide.len(), 0);
    assert!(empty_guide.is_empty());
}
