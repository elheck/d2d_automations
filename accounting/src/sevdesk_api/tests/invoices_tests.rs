//! Tests for invoice creation and position management.

use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::models::{OrderItem, OrderRecord};
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
        shipment_costs: "0,00".to_string(),
        total_value: "10,00".to_string(),
        commission: "1,00".to_string(),
        currency: "EUR".to_string(),
        description: "Lightning Bolt".to_string(),
        product_id: "12345".to_string(),
        localized_product_name: "Blitzschlag".to_string(),
        items: vec![OrderItem {
            description: "1x Lightning Bolt (Alpha) NM".to_string(),
            product_id: "12345".to_string(),
            localized_product_name: "Blitzschlag".to_string(),
            price: 10.0,
            quantity: 1,
        }],
    }
}

/// Mounts all dependency mocks needed by create_invoice_internal.
async fn mock_invoice_dependencies(mock_server: &MockServer) {
    // Contact search → found
    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{
                "id": "10",
                "name": "Test Customer",
                "objectName": "Contact",
                "customerNumber": null,
                "status": null
            }]
        })))
        .mount(mock_server)
        .await;

    // Current user
    Mock::given(method("GET"))
        .and(path("/SevUser"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{
                "id": "1",
                "username": "admin",
                "objectName": "SevUser"
            }]
        })))
        .mount(mock_server)
        .await;

    // Countries
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

// ── add_invoice_position ─────────────────────────────────────────────

#[tokio::test]
async fn add_invoice_position_success() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .and(header("Authorization", "test_token"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .mount(&mock_server)
        .await;

    let result = api
        .add_invoice_position("INV-100", 1, "Lightning Bolt", "Alpha NM", 2.0, 5.50)
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn add_invoice_position_500_error() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&mock_server)
        .await;

    // add_invoice_position currently returns Ok(()) even on 500 (only logs error)
    // This test documents that behavior
    let result = api
        .add_invoice_position("INV-100", 1, "Test", "Desc", 1.0, 5.0)
        .await;
    assert!(result.is_ok());
}

// ── create_invoice_internal ──────────────────────────────────────────

#[tokio::test]
async fn create_invoice_single_item() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    let order = create_test_order();

    mock_invoice_dependencies(&mock_server).await;

    // Invoice creation
    Mock::given(method("POST"))
        .and(path("/Invoice"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": {
                "id": "500",
                "invoiceNumber": "RE-2025-001"
            }
        })))
        .mount(&mock_server)
        .await;

    // Position creation (called once for single item)
    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .mount(&mock_server)
        .await;

    let (invoice_id, invoice_number) = api.create_invoice_internal(&order).await.unwrap();
    assert_eq!(invoice_id, "500");
    assert_eq!(invoice_number, "RE-2025-001");
}

#[tokio::test]
async fn create_invoice_multiple_items() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    let mut order = create_test_order();
    order.items = vec![
        OrderItem {
            description: "1x Lightning Bolt (Alpha) NM".to_string(),
            product_id: "111".to_string(),
            localized_product_name: "Blitzschlag".to_string(),
            price: 50.0,
            quantity: 1,
        },
        OrderItem {
            description: "2x Dark Ritual (Beta) NM".to_string(),
            product_id: "222".to_string(),
            localized_product_name: "Dunkles Ritual".to_string(),
            price: 5.0,
            quantity: 2,
        },
    ];
    order.merchandise_value = "60,00".to_string();
    order.total_value = "60,00".to_string();

    mock_invoice_dependencies(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/Invoice"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": {
                "id": "501",
                "invoiceNumber": "RE-2025-002"
            }
        })))
        .mount(&mock_server)
        .await;

    // Positions (called twice for two items)
    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .expect(2)
        .mount(&mock_server)
        .await;

    let (invoice_id, _) = api.create_invoice_internal(&order).await.unwrap();
    assert_eq!(invoice_id, "501");
}

#[tokio::test]
async fn create_invoice_with_shipping() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    let mut order = create_test_order();
    order.shipment_costs = "3,50".to_string();
    order.total_value = "13,50".to_string();

    mock_invoice_dependencies(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/Invoice"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": {
                "id": "502",
                "invoiceNumber": "RE-2025-003"
            }
        })))
        .mount(&mock_server)
        .await;

    // Positions: 1 item + 1 shipping = 2 calls
    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .expect(2)
        .mount(&mock_server)
        .await;

    let result = api.create_invoice_internal(&order).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn create_invoice_no_shipping() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    let order = create_test_order(); // shipment_costs = "0,00"

    mock_invoice_dependencies(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/Invoice"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": {
                "id": "503",
                "invoiceNumber": "RE-2025-004"
            }
        })))
        .mount(&mock_server)
        .await;

    // Only 1 position (no shipping)
    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let result = api.create_invoice_internal(&order).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn create_invoice_creation_fails() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    let order = create_test_order();

    mock_invoice_dependencies(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/Invoice"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Server Error"))
        .mount(&mock_server)
        .await;

    let result = api.create_invoice_internal(&order).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn create_invoice_contact_search_fails() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    let order = create_test_order();

    // Contact search returns broken JSON
    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&mock_server)
        .await;

    let result = api.create_invoice_internal(&order).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn create_invoice_fallback_no_items() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    let mut order = create_test_order();
    order.items = vec![]; // No parsed items — falls back to merchandise_value

    mock_invoice_dependencies(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/Invoice"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": {
                "id": "504",
                "invoiceNumber": "RE-2025-005"
            }
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .mount(&mock_server)
        .await;

    let result = api.create_invoice_internal(&order).await;
    assert!(result.is_ok());
}
