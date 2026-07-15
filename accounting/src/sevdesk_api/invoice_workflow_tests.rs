//! Tests for invoice workflow operations (finalize, enshrine, book, PDF download).

use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::models::{InvoiceWorkflowOptions, SendType};
use crate::sevdesk_api::SevDeskApi;

/// Creates a SevDeskApi pointing at the given mock server.
fn api_with_mock(mock_uri: &str) -> SevDeskApi {
    let mut api = SevDeskApi::new("test_token".to_string());
    api.base_url = mock_uri.to_string();
    api
}

// ── finalize_invoice ─────────────────────────────────────────────────

#[tokio::test]
async fn finalize_invoice_success_vpdf() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("PUT"))
        .and(path("/Invoice/100/sendBy"))
        .and(header("Authorization", "test_token"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&mock_server)
        .await;

    let result = api.finalize_invoice(100, &SendType::Vpdf).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn finalize_invoice_success_vpr() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("PUT"))
        .and(path("/Invoice/101/sendBy"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&mock_server)
        .await;

    let result = api.finalize_invoice(101, &SendType::Vpr).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn finalize_invoice_404_not_found() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("PUT"))
        .and(path("/Invoice/999/sendBy"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&mock_server)
        .await;

    let result = api.finalize_invoice(999, &SendType::Vpdf).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("404"),
        "Error should contain status code: {err}"
    );
}

#[tokio::test]
async fn finalize_invoice_500_server_error() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("PUT"))
        .and(path("/Invoice/100/sendBy"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&mock_server)
        .await;

    let result = api.finalize_invoice(100, &SendType::Vpdf).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("500"),
        "Error should contain status code: {err}"
    );
}

// ── enshrine_invoice ─────────────────────────────────────────────────

#[tokio::test]
async fn enshrine_invoice_success() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("PUT"))
        .and(path("/Invoice/100/enshrine"))
        .and(header("Authorization", "test_token"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&mock_server)
        .await;

    let result = api.enshrine_invoice(100).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn enshrine_invoice_400_bad_request() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("PUT"))
        .and(path("/Invoice/100/enshrine"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_string("Invoice is not in correct state for enshrining"),
        )
        .mount(&mock_server)
        .await;

    let result = api.enshrine_invoice(100).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn enshrine_invoice_404_not_found() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("PUT"))
        .and(path("/Invoice/999/enshrine"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&mock_server)
        .await;

    let result = api.enshrine_invoice(999).await;
    assert!(result.is_err());
}

// ── book_invoice ─────────────────────────────────────────────────────

/// Mounts the GET /Invoice/{id} mock that `get_invoice_amount` calls.
async fn mock_get_invoice_amount(mock_server: &MockServer, invoice_id: u32, amount: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/Invoice/{invoice_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": { "sumGross": amount }
        })))
        .mount(mock_server)
        .await;
}

/// Mounts the PUT /Invoice/{id}/bookAmount mock.
async fn mock_book_amount(mock_server: &MockServer, invoice_id: u32) {
    Mock::given(method("PUT"))
        .and(path(format!("/Invoice/{invoice_id}/bookAmount")))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(mock_server)
        .await;
}

#[tokio::test]
async fn book_invoice_success() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    mock_get_invoice_amount(&mock_server, 100, "50.00").await;
    mock_book_amount(&mock_server, 100).await;

    let result = api.book_invoice(100, "42", None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn book_invoice_with_german_date() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    mock_get_invoice_amount(&mock_server, 100, "25.00").await;
    mock_book_amount(&mock_server, 100).await;

    let result = api.book_invoice(100, "42", Some("15.01.2025")).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn book_invoice_with_iso_date() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    mock_get_invoice_amount(&mock_server, 100, "25.00").await;
    mock_book_amount(&mock_server, 100).await;

    let result = api.book_invoice(100, "42", Some("2025-01-15")).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn book_invoice_with_iso_datetime() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    mock_get_invoice_amount(&mock_server, 100, "25.00").await;
    mock_book_amount(&mock_server, 100).await;

    let result = api
        .book_invoice(100, "42", Some("2025-01-15 10:30:00"))
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn book_invoice_invalid_check_account_id() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    mock_get_invoice_amount(&mock_server, 100, "25.00").await;

    let result = api.book_invoice(100, "not-a-number", None).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Invalid check account ID"),
        "Should mention invalid ID: {err}"
    );
}

#[tokio::test]
async fn book_invoice_get_amount_fails() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/Invoice/100"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Server Error"))
        .mount(&mock_server)
        .await;

    let result = api.book_invoice(100, "42", None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn book_invoice_amount_as_number() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    // sumGross as a JSON number instead of string
    Mock::given(method("GET"))
        .and(path("/Invoice/100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": { "sumGross": 99.99 }
        })))
        .mount(&mock_server)
        .await;
    mock_book_amount(&mock_server, 100).await;

    let result = api.book_invoice(100, "42", None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn book_invoice_amount_from_array_response() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    // objects as array instead of object
    Mock::given(method("GET"))
        .and(path("/Invoice/100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{ "sumGross": "75.50" }]
        })))
        .mount(&mock_server)
        .await;
    mock_book_amount(&mock_server, 100).await;

    let result = api.book_invoice(100, "42", None).await;
    assert!(result.is_ok());
}

// ── download_invoice_pdf ─────────────────────────────────────────────

#[tokio::test]
async fn download_pdf_raw_bytes() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    let pdf_bytes = b"%PDF-1.4 fake pdf content here";

    Mock::given(method("GET"))
        .and(path("/Invoice/100/getPdf"))
        .and(query_param("download", "true"))
        .and(query_param("preventSendBy", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(pdf_bytes.to_vec()))
        .mount(&mock_server)
        .await;

    let tmp_dir = tempfile::tempdir().unwrap();
    let result = api
        .download_invoice_pdf(100, "INV-001", tmp_dir.path())
        .await;
    assert!(result.is_ok());

    let pdf_path = result.unwrap();
    assert_eq!(pdf_path.file_name().unwrap(), "INV-001.pdf");
    assert!(pdf_path.exists());

    let saved = std::fs::read(&pdf_path).unwrap();
    assert_eq!(saved, pdf_bytes);
}

#[tokio::test]
async fn download_pdf_base64_json() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    let original = b"fake pdf content";
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(original);

    Mock::given(method("GET"))
        .and(path("/Invoice/100/getPdf"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": { "content": encoded }
        })))
        .mount(&mock_server)
        .await;

    let tmp_dir = tempfile::tempdir().unwrap();
    let result = api
        .download_invoice_pdf(100, "INV-002", tmp_dir.path())
        .await;
    assert!(result.is_ok());

    let saved = std::fs::read(result.unwrap()).unwrap();
    assert_eq!(saved, original);
}

#[tokio::test]
async fn download_pdf_creates_directory() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/Invoice/100/getPdf"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"%PDF-1.4 test".to_vec()))
        .mount(&mock_server)
        .await;

    let tmp_dir = tempfile::tempdir().unwrap();
    let nested = tmp_dir.path().join("sub").join("dir");
    assert!(!nested.exists());

    let result = api.download_invoice_pdf(100, "INV-003", &nested).await;
    assert!(result.is_ok());
    assert!(nested.exists());
}

#[tokio::test]
async fn download_pdf_404_not_found() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/Invoice/999/getPdf"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&mock_server)
        .await;

    let tmp_dir = tempfile::tempdir().unwrap();
    let result = api
        .download_invoice_pdf(999, "INV-404", tmp_dir.path())
        .await;
    assert!(result.is_err());
}

// ── execute_invoice_workflow ─────────────────────────────────────────

#[tokio::test]
async fn workflow_finalize_only() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("PUT"))
        .and(path("/Invoice/100/sendBy"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&mock_server)
        .await;

    let options = InvoiceWorkflowOptions {
        finalize: true,
        send_type: SendType::Vpr, // Not VPDF so no PDF download
        ..Default::default()
    };

    let status = api.execute_invoice_workflow(100, "INV-001", &options).await;
    assert!(status.finalized);
    assert!(!status.enshrined);
    assert!(!status.booked);
    assert!(status.workflow_error.is_none());
}

#[tokio::test]
async fn workflow_enshrine_without_finalize_fails() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    let options = InvoiceWorkflowOptions {
        finalize: false,
        enshrine: true,
        ..Default::default()
    };

    let status = api.execute_invoice_workflow(100, "INV-001", &options).await;
    assert!(!status.enshrined);
    assert!(status.workflow_error.is_some());
    assert!(status.workflow_error.unwrap().contains("finalized first"));
}

#[tokio::test]
async fn workflow_book_without_check_account_fails() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    Mock::given(method("PUT"))
        .and(path("/Invoice/100/sendBy"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&mock_server)
        .await;

    let options = InvoiceWorkflowOptions {
        finalize: true,
        send_type: SendType::Vpr,
        book: true,
        check_account_id: None,
        ..Default::default()
    };

    let status = api.execute_invoice_workflow(100, "INV-001", &options).await;
    assert!(status.finalized);
    assert!(!status.booked);
    assert!(status.workflow_error.is_some());
    assert!(status.workflow_error.unwrap().contains("no check account"));
}

#[tokio::test]
async fn workflow_full_pipeline() {
    let mock_server = MockServer::start().await;
    let api = api_with_mock(&mock_server.uri());

    // Finalize
    Mock::given(method("PUT"))
        .and(path("/Invoice/100/sendBy"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&mock_server)
        .await;

    // Enshrine
    Mock::given(method("PUT"))
        .and(path("/Invoice/100/enshrine"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&mock_server)
        .await;

    // Book (get amount + book)
    mock_get_invoice_amount(&mock_server, 100, "50.00").await;
    mock_book_amount(&mock_server, 100).await;

    let options = InvoiceWorkflowOptions {
        finalize: true,
        send_type: SendType::Vpr,
        enshrine: true,
        book: true,
        check_account_id: Some("42".to_string()),
        ..Default::default()
    };

    let status = api.execute_invoice_workflow(100, "INV-001", &options).await;
    assert!(status.finalized);
    assert!(status.enshrined);
    assert!(status.booked);
    assert!(status.workflow_error.is_none());
}
