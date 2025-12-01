//! Tests for SevDeskApi construction.

use crate::sevdesk_api::SevDeskApi;

#[test]
fn creates_api_with_token() {
    let api = SevDeskApi::new("my_api_token".to_string());
    assert_eq!(api.api_token, "my_api_token");
    assert_eq!(api.base_url, "https://my.sevdesk.de/api/v1");
}

#[test]
fn creates_api_with_empty_token() {
    let api = SevDeskApi::new(String::new());
    assert!(api.api_token.is_empty());
}

#[test]
fn api_has_default_base_url() {
    let api = SevDeskApi::new("test_token".to_string());
    assert_eq!(api.base_url, "https://my.sevdesk.de/api/v1");
}
