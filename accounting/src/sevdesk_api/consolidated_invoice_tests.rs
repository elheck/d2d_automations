//! Tests for consolidated (CardTrader) invoice creation.

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::models::{ConsolidatedInvoice, ConsolidatedPosition, InvoiceRecipient};
use crate::sevdesk_api::SevDeskApi;

fn api_with_mock(mock_uri: &str) -> SevDeskApi {
    let mut api = SevDeskApi::new("test_token".to_string());
    api.base_url = mock_uri.to_string();
    api
}

fn test_invoice() -> ConsolidatedInvoice {
    ConsolidatedInvoice {
        recipient: InvoiceRecipient::default(),
        invoice_date: "2026-07-05".to_string(),
        period_label: "2026-07-01 - 2026-07-05".to_string(),
        positions: vec![
            ConsolidatedPosition {
                item_name: "Plains".to_string(),
                item_expansion: "Murders at Karlov Manor".to_string(),
                item_properties: "Moderately Played - DE".to_string(),
                quantity: 3,
                total_cents: 6,
            },
            ConsolidatedPosition {
                item_name: "Carnage Tyrant".to_string(),
                item_expansion: "Ixalan".to_string(),
                item_properties: "Slightly Played - EN".to_string(),
                quantity: 1,
                total_cents: 342,
            },
        ],
        currency: "EUR".to_string(),
        row_count: 4,
        skipped_row_count: 0,
    }
}

/// Mounts the dependencies needed to create a consolidated invoice.
async fn mock_dependencies(mock_server: &MockServer, contact_name: &str) {
    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{
                "id": "10",
                "name": contact_name,
                "objectName": "Contact",
                "customerNumber": null,
                "status": null
            }]
        })))
        .mount(mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/SevUser"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{
                "id": "7",
                "username": "tester",
                "objectName": "SevUser"
            }]
        })))
        .mount(mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/StaticCountry"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{
                "id": "15",
                "name": "Italien",
                "nameEn": "Italy",
                "translationCode": "IT",
                "locale": null,
                "priority": null
            }]
        })))
        .mount(mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/Invoice"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": { "id": "555", "invoiceNumber": "RE-1001" }
        })))
        .mount(mock_server)
        .await;
}

#[tokio::test]
async fn creates_invoice_with_one_position_per_group() {
    let mock_server = MockServer::start().await;
    mock_dependencies(&mock_server, "GRAY FOX SRL").await;

    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": { "id": "1" }
        })))
        .expect(2) // one call per consolidated position
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    let result = api
        .create_consolidated_invoice(&test_invoice())
        .await
        .expect("should return a result");

    assert!(
        result.error.is_none(),
        "unexpected error: {:?}",
        result.error
    );
    assert_eq!(result.invoice_id, Some(555));
    assert_eq!(result.invoice_number.as_deref(), Some("RE-1001"));
    assert_eq!(result.customer_name, "GRAY FOX SRL");
}

#[tokio::test]
async fn position_failure_aborts_instead_of_understating_the_invoice() {
    let mock_server = MockServer::start().await;
    mock_dependencies(&mock_server, "GRAY FOX SRL").await;

    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad position"))
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    let result = api
        .create_consolidated_invoice(&test_invoice())
        .await
        .expect("should return a result");

    let error = result
        .error
        .expect("position failure must surface as an error");
    assert!(
        error.contains("Failed to add position"),
        "unexpected error: {error}"
    );
    assert!(result.invoice_id.is_none());
}

#[tokio::test]
async fn invoice_creation_failure_is_reported() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{
                "id": "10",
                "name": "GRAY FOX SRL",
                "objectName": "Contact",
                "customerNumber": null,
                "status": null
            }]
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/SevUser"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{ "id": "7", "username": "tester", "objectName": "SevUser" }]
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/StaticCountry"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": []
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/Invoice"))
        .respond_with(ResponseTemplate::new(500).set_body_string("server error"))
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    let result = api
        .create_consolidated_invoice(&test_invoice())
        .await
        .expect("should return a result");

    assert!(result.error.is_some());
    assert!(result.invoice_id.is_none());
}

#[tokio::test]
async fn reuses_contact_only_on_exact_name_match() {
    let mock_server = MockServer::start().await;

    // SevDesk name search is fuzzy; a different company must not be reused.
    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{
                "id": "10",
                "name": "GRAY FOX SRL BRANCH OFFICE",
                "objectName": "Contact",
                "customerNumber": null,
                "status": null
            }]
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/StaticCountry"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{
                "id": "15", "name": "Italien", "nameEn": "Italy",
                "translationCode": "IT", "locale": null, "priority": null
            }]
        })))
        .mount(&mock_server)
        .await;

    // A new contact must be created instead of reusing the near-match.
    Mock::given(method("POST"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": { "id": "99", "name": "GRAY FOX SRL", "objectName": "Contact" }
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    let contact_id = api
        .get_or_create_recipient_contact(&InvoiceRecipient::default())
        .await
        .expect("should create contact");

    assert_eq!(contact_id, 99);
}

#[tokio::test]
async fn reuses_contact_when_name_matches_exactly() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{
                "id": "42",
                "name": "GRAY FOX SRL",
                "objectName": "Contact",
                "customerNumber": null,
                "status": null
            }]
        })))
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    let contact_id = api
        .get_or_create_recipient_contact(&InvoiceRecipient::default())
        .await
        .expect("should find contact");

    assert_eq!(contact_id, 42);
}

#[tokio::test]
async fn custom_recipient_is_used_for_contact_lookup() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{
                "id": "77",
                "name": "CUSTOM GMBH",
                "objectName": "Contact",
                "customerNumber": null,
                "status": null
            }]
        })))
        .mount(&mock_server)
        .await;

    let recipient = InvoiceRecipient {
        name: "CUSTOM GMBH".to_string(),
        street: "Teststraße 1".to_string(),
        zip: "10115".to_string(),
        city: "Berlin".to_string(),
        country: "Deutschland".to_string(),
    };

    let api = api_with_mock(&mock_server.uri());
    let contact_id = api
        .get_or_create_recipient_contact(&recipient)
        .await
        .expect("should find contact");

    assert_eq!(contact_id, 77);
}

#[tokio::test]
async fn dry_run_creates_nothing() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/StaticCountry"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{
                "id": "15", "name": "Italien", "nameEn": "Italy",
                "translationCode": "IT", "locale": null, "priority": null
            }]
        })))
        .mount(&mock_server)
        .await;

    // Any write would be an unmatched request; assert none happen below.
    let api = api_with_mock(&mock_server.uri());
    let result = api
        .simulate_consolidated_invoice(&test_invoice())
        .await
        .expect("should simulate");

    assert!(result.error.is_none());
    assert_eq!(result.invoice_id, Some(99999));
    assert_eq!(
        result.invoice_number.as_deref(),
        Some("DRY-CARDTRADER-2026-07-05")
    );

    let writes = mock_server
        .received_requests()
        .await
        .expect("requests recorded")
        .into_iter()
        .filter(|r| r.method == wiremock::http::Method::POST)
        .count();
    assert_eq!(writes, 0, "dry run must not POST anything");
}

#[tokio::test]
async fn dry_run_rejects_invoice_without_positions() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/StaticCountry"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": []
        })))
        .mount(&mock_server)
        .await;

    let mut invoice = test_invoice();
    invoice.positions.clear();

    let api = api_with_mock(&mock_server.uri());
    let result = api
        .simulate_consolidated_invoice(&invoice)
        .await
        .expect("should return a result");

    assert!(result.error.is_some());
    assert!(result.invoice_id.is_none());
}
