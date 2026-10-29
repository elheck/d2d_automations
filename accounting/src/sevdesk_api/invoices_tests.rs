//! Tests for invoice creation and position management.

use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::models::{OrderItem, OrderRecord};
use crate::sevdesk_api::invoices::{order_expected_cents, plan_order_positions};
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
        country: "Germany".to_string(),
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

/// Mounts a successful invoice creation returning the given ID and number.
async fn mock_invoice_created(mock_server: &MockServer, id: &str, number: &str) {
    Mock::given(method("POST"))
        .and(path("/Invoice"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": { "id": id, "invoiceNumber": number }
        })))
        .mount(mock_server)
        .await;
}

fn two_item_order() -> OrderRecord {
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
    order
}

// ── plan_order_positions ─────────────────────────────────────────────

#[test]
fn plan_has_one_position_per_item() {
    let positions = plan_order_positions(&two_item_order()).unwrap();
    assert_eq!(positions.len(), 2);
    assert_eq!(positions[0].number, 1);
    assert_eq!(positions[0].name, "Blitzschlag");
    assert_eq!(positions[1].number, 2);
    assert_eq!(positions[1].quantity, 2.0);
    assert_eq!(positions[1].unit_price, 5.0);
    assert_eq!(positions[1].text, "2x Dark Ritual (Beta) NM");
}

#[test]
fn plan_adds_shipping_as_last_position() {
    let mut order = create_test_order();
    order.shipment_costs = "3,50".to_string();
    let positions = plan_order_positions(&order).unwrap();
    assert_eq!(positions.len(), 2);
    assert_eq!(positions[1].number, 2);
    assert_eq!(positions[1].name, "Shipping");
    assert_eq!(positions[1].unit_price, 3.5);
}

#[test]
fn plan_without_shipping_has_no_shipping_position() {
    let positions = plan_order_positions(&create_test_order()).unwrap();
    assert_eq!(positions.len(), 1);
}

#[test]
fn plan_fallback_bills_merchandise_value_once() {
    let mut order = create_test_order();
    order.items.clear();
    order.article_count = 3;
    order.merchandise_value = "9,00".to_string();
    let positions = plan_order_positions(&order).unwrap();
    assert_eq!(positions.len(), 1);
    // Not 3 x 9.00: the merchandise value already covers all three articles.
    assert_eq!(positions[0].quantity, 1.0);
    assert_eq!(positions[0].unit_price, 9.0);
}

#[test]
fn plan_rejects_unparseable_prices() {
    let mut order = create_test_order();
    order.shipment_costs = "abc".to_string();
    assert!(plan_order_positions(&order).is_err());
}

#[test]
fn expected_total_comes_from_order_total() {
    let mut order = create_test_order();
    order.total_value = "13,50".to_string();
    assert_eq!(order_expected_cents(&order).unwrap(), 1350);
    order.total_value = String::new();
    assert!(order_expected_cents(&order).is_err());
}

// ── create_order_draft ───────────────────────────────────────────────

#[tokio::test]
async fn draft_single_item() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    mock_invoice_dependencies(&mock_server).await;
    mock_invoice_created(&mock_server, "500", "RE-2025-001").await;

    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .and(header("Authorization", "test_token"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let draft = api.create_order_draft(&create_test_order()).await.unwrap();
    assert_eq!(draft.invoice_id, 500);
    assert_eq!(draft.invoice_number, "RE-2025-001");
    assert_eq!(draft.expected_cents, 1000);
    assert_eq!(draft.planned_cents, 1000);
    assert!(draft.failed.is_empty());
}

#[tokio::test]
async fn draft_multiple_items() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    mock_invoice_dependencies(&mock_server).await;
    mock_invoice_created(&mock_server, "501", "RE-2025-002").await;

    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .expect(2)
        .mount(&mock_server)
        .await;

    let draft = api.create_order_draft(&two_item_order()).await.unwrap();
    assert_eq!(draft.invoice_id, 501);
    assert_eq!(draft.planned_cents, 6000);
}

#[tokio::test]
async fn draft_with_shipping() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    mock_invoice_dependencies(&mock_server).await;
    mock_invoice_created(&mock_server, "502", "RE-2025-003").await;

    let mut order = create_test_order();
    order.shipment_costs = "3,50".to_string();
    order.total_value = "13,50".to_string();

    // Positions: 1 item + 1 shipping = 2 calls
    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .expect(2)
        .mount(&mock_server)
        .await;

    let draft = api.create_order_draft(&order).await.unwrap();
    assert_eq!(draft.expected_cents, 1350);
    assert_eq!(draft.planned_cents, 1350);
}

#[tokio::test]
async fn draft_records_rejected_positions_instead_of_hiding_them() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    mock_invoice_dependencies(&mock_server).await;
    mock_invoice_created(&mock_server, "503", "RE-2025-004").await;

    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .expect(2) // every position is still attempted
        .mount(&mock_server)
        .await;

    let draft = api.create_order_draft(&two_item_order()).await.unwrap();
    assert_eq!(draft.failed.len(), 2);
    assert_eq!(draft.failed[0].position.name, "Blitzschlag");
    assert!(draft.failed[0].error.contains("500"));
}

#[tokio::test]
async fn draft_creation_fails() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    mock_invoice_dependencies(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/Invoice"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Server Error"))
        .mount(&mock_server)
        .await;

    assert!(api.create_order_draft(&create_test_order()).await.is_err());
}

#[tokio::test]
async fn draft_contact_search_fails() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    // Contact search returns broken JSON
    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&mock_server)
        .await;

    assert!(api.create_order_draft(&create_test_order()).await.is_err());
}

#[tokio::test]
async fn draft_with_bad_prices_writes_nothing() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    let mut order = create_test_order();
    order.total_value = "not a price".to_string();

    assert!(api.create_order_draft(&order).await.is_err());
    assert!(
        mock_server.received_requests().await.unwrap().is_empty(),
        "bad order data must fail before any API call"
    );
}

#[tokio::test]
async fn draft_with_unknown_country_fails() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());
    mock_invoice_dependencies(&mock_server).await;

    let mut order = create_test_order();
    order.country = "Niger".to_string();

    let error = format!("{:#}", api.create_order_draft(&order).await.unwrap_err());
    assert!(error.contains("Unknown country 'Niger'"), "{error}");
}
