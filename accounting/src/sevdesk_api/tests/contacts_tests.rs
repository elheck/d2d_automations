//! Tests for contact management (get_or_create_contact).

use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::models::OrderRecord;
use crate::sevdesk_api::SevDeskApi;

fn api_with_mock(mock_uri: &str) -> SevDeskApi {
    let mut api = SevDeskApi::new("test_token".to_string());
    api.base_url = mock_uri.to_string();
    api
}

fn create_test_order() -> OrderRecord {
    OrderRecord {
        order_id: "ORD-001".to_string(),
        username: "testuser".to_string(),
        name: "Test Customer".to_string(),
        street: "Hauptstraße 42".to_string(),
        zip: "10115".to_string(),
        city: "Berlin".to_string(),
        country: "Deutschland".to_string(),
        is_professional: None,
        vat_number: None,
        date_of_purchase: "2025-01-15 10:30:00".to_string(),
        article_count: 1,
        merchandise_value: "10,00".to_string(),
        shipment_costs: "2,00".to_string(),
        total_value: "12,00".to_string(),
        commission: "1,20".to_string(),
        currency: "EUR".to_string(),
        description: "Test Card".to_string(),
        product_id: "12345".to_string(),
        localized_product_name: "Testkarte".to_string(),
        items: vec![],
    }
}

/// Mounts the StaticCountry response that get_country_id needs.
async fn mock_countries(mock_server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/StaticCountry"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [
                { "id": "1", "name": "Deutschland", "nameEn": "Germany", "translationCode": null, "locale": null, "priority": null }
            ]
        })))
        .mount(mock_server)
        .await;
}

// ── get_or_create_contact ────────────────────────────────────────────

#[tokio::test]
async fn find_existing_contact() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    let order = create_test_order();

    Mock::given(method("GET"))
        .and(path("/Contact"))
        .and(query_param("name", "Test Customer"))
        .and(header("Authorization", "test_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{
                "id": "123",
                "name": "Test Customer",
                "objectName": "Contact",
                "customerNumber": null,
                "status": null
            }]
        })))
        .mount(&mock_server)
        .await;

    let contact_id = api.get_or_create_contact(&order).await.unwrap();
    assert_eq!(contact_id, 123);
}

#[tokio::test]
async fn create_contact_when_search_returns_empty() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    let order = create_test_order();

    // Search returns empty array
    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": []
        })))
        .mount(&mock_server)
        .await;

    // Country lookup for create
    mock_countries(&mock_server).await;

    // Create returns new contact
    Mock::given(method("POST"))
        .and(path("/Contact"))
        .and(header("Authorization", "test_token"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": {
                "id": "456",
                "name": "Test Customer",
                "objectName": "Contact",
                "customerNumber": null,
                "status": null
            }
        })))
        .mount(&mock_server)
        .await;

    let contact_id = api.get_or_create_contact(&order).await.unwrap();
    assert_eq!(contact_id, 456);
}

#[tokio::test]
async fn create_contact_when_objects_null() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    let order = create_test_order();

    // Search returns null objects
    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": null
        })))
        .mount(&mock_server)
        .await;

    mock_countries(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": {
                "id": "789",
                "name": "Test Customer",
                "objectName": "Contact",
                "customerNumber": null,
                "status": null
            }
        })))
        .mount(&mock_server)
        .await;

    let contact_id = api.get_or_create_contact(&order).await.unwrap();
    assert_eq!(contact_id, 789);
}

#[tokio::test]
async fn create_contact_500_error() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    let order = create_test_order();

    // Search returns empty
    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": []
        })))
        .mount(&mock_server)
        .await;

    mock_countries(&mock_server).await;

    // Create fails with 500
    Mock::given(method("POST"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&mock_server)
        .await;

    let result = api.get_or_create_contact(&order).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn search_contact_malformed_json() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    let order = create_test_order();

    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_string("this is not json"))
        .mount(&mock_server)
        .await;

    let result = api.get_or_create_contact(&order).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn find_contact_with_different_country() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    let mut order = create_test_order();
    order.name = "French Customer".to_string();
    order.country = "France".to_string();

    // Search returns empty — need to create
    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": []
        })))
        .mount(&mock_server)
        .await;

    // Country lookup includes France
    Mock::given(method("GET"))
        .and(path("/StaticCountry"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [
                { "id": "1", "name": "Deutschland", "nameEn": "Germany", "translationCode": null, "locale": null, "priority": null },
                { "id": "5", "name": "Frankreich", "nameEn": "France", "translationCode": null, "locale": null, "priority": null }
            ]
        })))
        .mount(&mock_server)
        .await;

    // Create succeeds
    Mock::given(method("POST"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": {
                "id": "555",
                "name": "French Customer",
                "objectName": "Contact",
                "customerNumber": null,
                "status": null
            }
        })))
        .mount(&mock_server)
        .await;

    let contact_id = api.get_or_create_contact(&order).await.unwrap();
    assert_eq!(contact_id, 555);
}
