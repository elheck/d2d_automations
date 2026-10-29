//! Tests for country lookup and the country cache.

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::models::StaticCountryResponse;
use crate::sevdesk_api::countries::{normalize_country_name, CountryCache, CountryOption};
use crate::sevdesk_api::SevDeskApi;

fn country(id: &str, name: &str, name_en: Option<&str>) -> StaticCountryResponse {
    StaticCountryResponse {
        id: id.to_string(),
        name: name.to_string(),
        name_en: name_en.map(str::to_string),
        translation_code: None,
        locale: None,
        priority: None,
    }
}

fn sample_countries() -> Vec<StaticCountryResponse> {
    vec![
        country("1", "Deutschland", Some("Germany")),
        country("9", "Großbritannien", Some("Great Britain")),
        country("50", "Nigeria", Some("Nigeria")),
        country("5", "Frankreich", Some("France")),
    ]
}

fn api_with_mock(mock_uri: &str) -> SevDeskApi {
    let mut api = SevDeskApi::new("test_token".to_string());
    api.base_url = mock_uri.to_string();
    api
}

async fn mount_countries(mock_server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/StaticCountry"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [
                { "id": "1", "name": "Deutschland", "nameEn": "Germany", "translationCode": null, "locale": null, "priority": null },
                { "id": "50", "name": "Nigeria", "nameEn": "Nigeria", "translationCode": null, "locale": null, "priority": null }
            ]
        })))
        .mount(mock_server)
        .await;
}

#[test]
fn country_cache_default_is_not_loaded() {
    let cache = CountryCache::default();
    assert!(!cache.loaded);
    assert!(cache.name_to_id.is_empty());
}

#[test]
fn normalize_trims_and_lowercases() {
    assert_eq!(normalize_country_name("  Germany "), "germany");
    assert_eq!(normalize_country_name("GROSSBRITANNIEN"), "grossbritannien");
}

#[test]
fn lookup_matches_german_and_english_names_case_insensitively() {
    let cache = CountryCache::from_countries(&sample_countries()).unwrap();
    assert!(cache.loaded);
    assert_eq!(cache.lookup("Deutschland"), Some(1));
    assert_eq!(cache.lookup("germany"), Some(1));
    assert_eq!(cache.lookup("  FRANCE "), Some(5));
}

#[test]
fn lookup_never_matches_partial_names() {
    let cache = CountryCache::from_countries(&sample_countries()).unwrap();
    // "Niger" is a substring of "Nigeria" but a different country.
    assert_eq!(cache.lookup("Niger"), None);
    assert_eq!(cache.lookup("Deutsch"), None);
    assert_eq!(cache.lookup("Republic of France"), None);
}

#[test]
fn unknown_country_is_none_not_germany() {
    let cache = CountryCache::from_countries(&sample_countries()).unwrap();
    assert_eq!(cache.lookup("Atlantis"), None);
    assert_eq!(cache.lookup(""), None);
}

#[test]
fn fixed_aliases_resolve() {
    let cache = CountryCache::from_countries(&sample_countries()).unwrap();
    assert_eq!(cache.lookup("DE"), Some(1));
    assert_eq!(cache.lookup("UK"), Some(9));
    assert_eq!(cache.lookup("great britain"), Some(9));
    assert_eq!(cache.lookup("großbritannien"), Some(9));
}

#[test]
fn remember_adds_a_name() {
    let mut cache = CountryCache::from_countries(&sample_countries()).unwrap();
    assert_eq!(cache.lookup("United Kingdom"), None);
    cache.remember("United Kingdom", 9);
    assert_eq!(cache.lookup("united kingdom"), Some(9));
}

#[test]
fn remember_ignores_blank_names() {
    let mut cache = CountryCache::default();
    cache.remember("   ", 9);
    assert!(cache.name_to_id.is_empty());
}

#[test]
fn options_are_sorted_and_show_both_names() {
    let cache = CountryCache::from_countries(&sample_countries()).unwrap();
    let names: Vec<&str> = cache
        .options
        .iter()
        .map(|o| o.display_name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "France (Frankreich)",
            "Germany (Deutschland)",
            "Great Britain (Großbritannien)",
            // Same German and English name: shown once.
            "Nigeria",
        ]
    );
    assert_eq!(
        cache.options[1],
        CountryOption {
            id: 1,
            display_name: "Germany (Deutschland)".to_string()
        }
    );
}

#[test]
fn invalid_country_id_is_an_error() {
    let result = CountryCache::from_countries(&[country("abc", "Nowhere", None)]);
    assert!(result.is_err());
}

#[tokio::test]
async fn get_country_id_fails_for_unknown_country() {
    let mock_server = MockServer::start().await;
    mount_countries(&mock_server).await;
    let api = api_with_mock(&mock_server.uri());

    let error = api.get_country_id("Niger").await.unwrap_err().to_string();
    assert!(error.contains("Unknown country 'Niger'"), "{error}");
    assert_eq!(api.get_country_id("Germany").await.unwrap(), 1);
}

#[tokio::test]
async fn lookup_country_id_returns_none_for_unknown_country() {
    let mock_server = MockServer::start().await;
    mount_countries(&mock_server).await;
    let api = api_with_mock(&mock_server.uri());

    assert_eq!(api.lookup_country_id("Atlantis").await.unwrap(), None);
    assert_eq!(api.lookup_country_id("nigeria").await.unwrap(), Some(50));
}

#[tokio::test]
async fn remembered_country_is_used_for_later_lookups() {
    let mock_server = MockServer::start().await;
    mount_countries(&mock_server).await;
    let api = api_with_mock(&mock_server.uri());

    assert_eq!(api.lookup_country_id("Bundesrepublik").await.unwrap(), None);
    api.remember_country("Bundesrepublik", 1).await;
    assert_eq!(api.get_country_id("bundesrepublik").await.unwrap(), 1);
}

#[tokio::test]
async fn country_options_come_from_sevdesk() {
    let mock_server = MockServer::start().await;
    mount_countries(&mock_server).await;
    let api = api_with_mock(&mock_server.uri());

    let options = api.country_options().await.unwrap();
    assert_eq!(options.len(), 2);
    assert_eq!(options[0].display_name, "Germany (Deutschland)");
}

#[tokio::test]
async fn failed_country_fetch_is_an_error() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/StaticCountry"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;
    let api = api_with_mock(&mock_server.uri());

    assert!(api.lookup_country_id("Germany").await.is_err());
}
