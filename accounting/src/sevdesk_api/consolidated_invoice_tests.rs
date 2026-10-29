//! Tests for consolidated (CardTrader) invoice creation.

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::models::{ConsolidatedInvoice, ConsolidatedPosition, InvoiceRecipient};
use crate::sevdesk_api::consolidated_invoice::plan_consolidated_positions;
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
    let draft = api
        .create_consolidated_draft(&test_invoice())
        .await
        .expect("should create a draft");

    assert!(draft.failed.is_empty());
    assert_eq!(draft.invoice_id, 555);
    assert_eq!(draft.invoice_number, "RE-1001");
    assert_eq!(draft.expected_cents, 348);
    assert_eq!(draft.planned_cents, 348);
}

#[test]
fn plan_has_one_position_per_group() {
    let positions = plan_consolidated_positions(&test_invoice());
    assert_eq!(positions.len(), 2);
    assert_eq!(positions[0].number, 1);
    assert_eq!(positions[0].name, "Plains (Murders at Karlov Manor)");
    assert_eq!(positions[0].text, "Moderately Played - DE");
    assert_eq!(positions[0].quantity, 3.0);
    assert_eq!(positions[0].total_cents(), 6);
    assert_eq!(positions[1].number, 2);
    assert_eq!(positions[1].total_cents(), 342);
}

#[tokio::test]
async fn position_failure_is_recorded_instead_of_understating_the_invoice() {
    let mock_server = MockServer::start().await;
    mock_dependencies(&mock_server, "GRAY FOX SRL").await;

    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad position"))
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    let draft = api
        .create_consolidated_draft(&test_invoice())
        .await
        .expect("the draft exists even though positions failed");

    // Both positions were attempted and both failures are kept for retrying.
    assert_eq!(draft.failed.len(), 2);
    assert!(draft.failed[0].error.contains("bad position"));
    assert_eq!(draft.invoice_id, 555);
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
            "objects": [{
                "id": "15", "name": "Italien", "nameEn": "Italy",
                "translationCode": "IT", "locale": null, "priority": null
            }]
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/Invoice"))
        .respond_with(ResponseTemplate::new(500).set_body_string("server error"))
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    let error = api
        .create_consolidated_draft(&test_invoice())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("Failed to create invoice"), "{error}");
}

#[tokio::test]
async fn draft_without_positions_writes_nothing() {
    let mock_server = MockServer::start().await;
    let mut invoice = test_invoice();
    invoice.positions.clear();

    let api = api_with_mock(&mock_server.uri());
    assert!(api.create_consolidated_draft(&invoice).await.is_err());
    assert!(mock_server.received_requests().await.unwrap().is_empty());
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
            "objects": [{
                "id": "15", "name": "Italien", "nameEn": "Italy",
                "translationCode": "IT", "locale": null, "priority": null
            }]
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

    let error = result.error.expect("must be an error");
    assert!(error.contains("no positions"), "{error}");
    assert!(result.invoice_id.is_none());
}

#[tokio::test]
async fn dry_run_reports_unknown_recipient_country() {
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

    let mut invoice = test_invoice();
    invoice.recipient.country = "Itallien".to_string();

    let api = api_with_mock(&mock_server.uri());
    let result = api
        .simulate_consolidated_invoice(&invoice)
        .await
        .expect("should return a result");

    let error = result.error.expect("unknown country must be an error");
    assert!(error.contains("Unknown country 'Itallien'"), "{error}");
}
