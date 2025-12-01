//! Tests for price parsing utility.

use crate::sevdesk_api::SevDeskApi;

#[test]
fn parses_comma_decimal() {
    let api = SevDeskApi::new("test_token".to_string());
    let price = api.parse_price("5,00").unwrap();
    assert!((price - 5.0).abs() < 0.001);
}

#[test]
fn parses_dot_decimal() {
    let api = SevDeskApi::new("test_token".to_string());
    let price = api.parse_price("5.00").unwrap();
    assert!((price - 5.0).abs() < 0.001);
}

#[test]
fn parses_integer() {
    let api = SevDeskApi::new("test_token".to_string());
    let price = api.parse_price("100").unwrap();
    assert!((price - 100.0).abs() < 0.001);
}

#[test]
fn parses_zero() {
    let api = SevDeskApi::new("test_token".to_string());
    let price = api.parse_price("0").unwrap();
    assert!((price - 0.0).abs() < 0.001);
}

#[test]
fn parses_decimal_cents() {
    let api = SevDeskApi::new("test_token".to_string());
    let price = api.parse_price("1,87").unwrap();
    assert!((price - 1.87).abs() < 0.001);
}

#[test]
fn parses_large_price() {
    let api = SevDeskApi::new("test_token".to_string());
    let price = api.parse_price("1234,56").unwrap();
    assert!((price - 1234.56).abs() < 0.001);
}

#[test]
fn fails_for_invalid_input() {
    let api = SevDeskApi::new("test_token".to_string());
    let result = api.parse_price("not a number");
    assert!(result.is_err());
}

#[test]
fn fails_for_empty_string() {
    let api = SevDeskApi::new("test_token".to_string());
    let result = api.parse_price("");
    assert!(result.is_err());
}
