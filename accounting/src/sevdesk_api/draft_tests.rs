//! Tests for draft invoices: line items, totals and verification.

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::sevdesk_api::draft::{
    check_draft, format_cents, parse_sum_gross, planned_total_cents, to_cents, DraftProblem,
    FailedPosition, InvoiceDraft, PlannedPosition,
};
use crate::sevdesk_api::SevDeskApi;

fn api_with_mock(mock_uri: &str) -> SevDeskApi {
    let mut api = SevDeskApi::new("test_token".to_string());
    api.base_url = mock_uri.to_string();
    api
}

fn position(number: u32, quantity: f64, unit_price: f64) -> PlannedPosition {
    PlannedPosition {
        number,
        name: format!("Card {number}"),
        text: "NM".to_string(),
        quantity,
        unit_price,
    }
}

fn draft(expected_cents: i64, planned_cents: i64, failed: Vec<FailedPosition>) -> InvoiceDraft {
    InvoiceDraft {
        invoice_id: 500,
        invoice_number: "RE-1".to_string(),
        expected_cents,
        planned_cents,
        failed,
    }
}

fn failed(number: u32) -> FailedPosition {
    FailedPosition {
        position: position(number, 1.0, 2.5),
        error: "500 Internal Server Error".to_string(),
    }
}

// ── amounts ──────────────────────────────────────────────────────────

#[test]
fn to_cents_rounds_float_noise() {
    assert_eq!(to_cents(0.1 + 0.2), 30);
    assert_eq!(to_cents(12.345), 1235);
    assert_eq!(to_cents(-5.0), -500);
}

#[test]
fn format_cents_uses_two_decimals_and_sign() {
    assert_eq!(format_cents(1250), "12.50");
    assert_eq!(format_cents(7), "0.07");
    assert_eq!(format_cents(0), "0.00");
    assert_eq!(format_cents(-500), "-5.00");
    assert_eq!(format_cents(-7), "-0.07");
}

#[test]
fn position_total_multiplies_quantity_and_price() {
    assert_eq!(position(1, 2.0, 2.75).total_cents(), 550);
    // CardTrader groups can have fractional-cent unit prices.
    assert_eq!(position(1, 2.0, 0.035).total_cents(), 7);
}

#[test]
fn planned_total_sums_all_positions() {
    let positions = vec![position(1, 2.0, 2.75), position(2, 1.0, 1.50)];
    assert_eq!(planned_total_cents(&positions), 700);
    assert_eq!(planned_total_cents(&[]), 0);
}

// ── check_draft ──────────────────────────────────────────────────────

#[test]
fn matching_total_passes() {
    assert_eq!(check_draft(&draft(1000, 1000, vec![]), Ok(1000)), None);
}

#[test]
fn matching_total_passes_even_if_a_request_failed() {
    // E.g. a timed-out request whose position SevDesk saved anyway.
    assert_eq!(
        check_draft(&draft(1000, 1000, vec![failed(2)]), Ok(1000)),
        None
    );
}

#[test]
fn missing_line_item_is_a_problem() {
    let problem = check_draft(&draft(1000, 1000, vec![failed(2)]), Ok(750)).unwrap();
    assert_eq!(problem.expected_cents, 1000);
    assert_eq!(problem.actual_cents, Ok(750));
    assert_eq!(problem.failed.len(), 1);
    assert!(!problem.line_items_disagree_with_total());
}

#[test]
fn total_above_expected_is_a_problem() {
    // E.g. a retried position that SevDesk had already saved.
    assert!(check_draft(&draft(1000, 1000, vec![]), Ok(1250)).is_some());
}

#[test]
fn unreadable_total_is_a_problem() {
    let problem = check_draft(&draft(1000, 1000, vec![]), Err("timeout".to_string())).unwrap();
    assert_eq!(problem.actual_cents, Err("timeout".to_string()));
}

#[test]
fn line_items_that_do_not_add_up_are_flagged_as_data_problem() {
    let problem = check_draft(&draft(1200, 1000, vec![]), Ok(1000)).unwrap();
    assert!(problem.line_items_disagree_with_total());
}

#[test]
fn summary_names_totals_and_rejected_items() {
    let problem = DraftProblem {
        expected_cents: 1000,
        planned_cents: 1000,
        actual_cents: Ok(750),
        failed: vec![failed(2)],
    };
    assert_eq!(
        problem.summary(),
        "SevDesk total 7.50 instead of 10.00, 1 line item(s) rejected"
    );

    let data_problem = DraftProblem {
        expected_cents: 1200,
        planned_cents: 1000,
        actual_cents: Ok(1000),
        failed: vec![],
    };
    assert_eq!(
        data_problem.summary(),
        "SevDesk total 10.00 instead of 12.00, the order's line items add up to 10.00"
    );

    let unreadable = DraftProblem {
        expected_cents: 1000,
        planned_cents: 1000,
        actual_cents: Err("timeout".to_string()),
        failed: vec![],
    };
    assert_eq!(
        unreadable.summary(),
        "SevDesk total could not be read (timeout)"
    );
}

// ── parse_sum_gross ──────────────────────────────────────────────────

#[test]
fn parse_sum_gross_reads_string_number_and_array() {
    let as_string = serde_json::json!({ "objects": { "sumGross": "12.50" } });
    let as_number = serde_json::json!({ "objects": { "sumGross": 12.5 } });
    let as_array = serde_json::json!({ "objects": [{ "sumGross": "12.50" }] });
    assert_eq!(parse_sum_gross(&as_string), Some(12.5));
    assert_eq!(parse_sum_gross(&as_number), Some(12.5));
    assert_eq!(parse_sum_gross(&as_array), Some(12.5));
}

#[test]
fn parse_sum_gross_missing_is_none() {
    assert_eq!(parse_sum_gross(&serde_json::json!({ "objects": {} })), None);
    assert_eq!(parse_sum_gross(&serde_json::json!({ "objects": [] })), None);
    assert_eq!(
        parse_sum_gross(&serde_json::json!({ "objects": { "sumGross": "n/a" } })),
        None
    );
}

// ── API calls ────────────────────────────────────────────────────────

#[tokio::test]
async fn add_invoice_position_fails_on_rejection() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad position"))
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    let error = api
        .add_invoice_position(500, &position(1, 1.0, 2.0))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("400") && error.contains("bad position"),
        "{error}"
    );
}

#[tokio::test]
async fn add_invoice_position_sends_kleinunternehmer_prices() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    api.add_invoice_position(500, &position(3, 2.0, 5.5))
        .await
        .unwrap();

    let request = &mock_server.received_requests().await.unwrap()[0];
    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["invoice"]["id"], "500");
    assert_eq!(body["positionNumber"], 3);
    assert_eq!(body["quantity"], 2.0);
    assert_eq!(body["priceGross"], 5.5);
    assert_eq!(body["taxRate"], 0.0);
}

#[tokio::test]
async fn add_positions_collects_every_failure_and_keeps_going() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(500).set_body_string("down"))
        .expect(3) // stops at nothing: every position is attempted
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    let positions = vec![
        position(1, 1.0, 1.0),
        position(2, 1.0, 2.0),
        position(3, 1.0, 3.0),
    ];
    let failed = api.add_positions(500, &positions).await;
    assert_eq!(failed.len(), 3);
    assert_eq!(failed[1].position, positions[1]);
    assert!(failed[1].error.contains("down"));
}

#[tokio::test]
async fn add_positions_returns_nothing_when_all_succeed() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    assert!(api
        .add_positions(500, &[position(1, 1.0, 1.0)])
        .await
        .is_empty());
}

#[tokio::test]
async fn fetch_invoice_total_reads_cents() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Invoice/500"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{ "id": "500", "sumGross": "13.50" }]
        })))
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    assert_eq!(api.fetch_invoice_total_cents(500).await.unwrap(), 1350);
}

#[tokio::test]
async fn fetch_invoice_total_without_sum_is_an_error() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Invoice/500"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{ "id": "500" }]
        })))
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    assert!(api.fetch_invoice_total_cents(500).await.is_err());
}

#[tokio::test]
async fn verify_draft_compares_sevdesk_total() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Invoice/500"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{ "id": "500", "sumGross": "7.50" }]
        })))
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    assert_eq!(api.verify_draft(&draft(750, 750, vec![])).await, None);
    let problem = api.verify_draft(&draft(1000, 1000, vec![])).await.unwrap();
    assert_eq!(problem.actual_cents, Ok(750));
}

#[tokio::test]
async fn verify_draft_reports_unreadable_total() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Invoice/500"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    let problem = api.verify_draft(&draft(1000, 1000, vec![])).await.unwrap();
    assert!(problem.actual_cents.is_err());
}

#[tokio::test]
async fn delete_invoice_success_and_failure() {
    let mock_server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/Invoice/500"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&mock_server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/Invoice/501"))
        .respond_with(ResponseTemplate::new(403).set_body_string("not a draft"))
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    assert!(api.delete_invoice(500).await.is_ok());
    let error = api.delete_invoice(501).await.unwrap_err().to_string();
    assert!(error.contains("not a draft"), "{error}");
}

#[tokio::test]
async fn post_invoice_rejects_non_numeric_id() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/Invoice"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": { "id": "abc", "invoiceNumber": "RE-1" }
        })))
        .mount(&mock_server)
        .await;

    let api = api_with_mock(&mock_server.uri());
    let invoice = crate::models::SevDeskInvoice {
        invoice_number: None,
        contact: crate::models::SevDeskContactRef {
            id: 1,
            object_name: "Contact".to_string(),
        },
        invoice_date: "2025-01-15".to_string(),
        header: "Test".to_string(),
        head_text: None,
        foot_text: None,
        address: None,
        address_country: crate::models::SevDeskCountry {
            id: 1,
            object_name: "StaticCountry".to_string(),
        },
        delivery_date: "2025-01-15".to_string(),
        status: 100,
        small_settlement: false,
        contact_person: crate::models::SevDeskUser {
            id: 1,
            object_name: "SevUser".to_string(),
        },
        tax_rate: 0.0,
        tax_text: String::new(),
        tax_rule: crate::models::SevDeskTaxRule {
            id: 11,
            object_name: "TaxRule".to_string(),
        },
        dunning_level: None,
        invoice_type: "RE".to_string(),
        currency: "EUR".to_string(),
    };
    assert!(api.post_invoice(&invoice).await.is_err());
}
