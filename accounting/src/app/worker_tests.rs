//! Tests for the background invoice worker, against a mock SevDesk.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{run_invoice_jobs, InvoiceJob};
use crate::app::decision::{CountryDecision, DecisionRequest, InvoiceDecision};
use crate::app::progress::ProgressEvent;
use crate::models::{
    ConsolidatedInvoice, ConsolidatedPosition, InvoiceCreationResult, InvoiceRecipient,
    InvoiceWorkflowOptions, OrderRecord,
};
use crate::sevdesk_api::SevDeskApi;

fn test_order(order_id: &str, name: &str) -> OrderRecord {
    OrderRecord {
        order_id: order_id.to_string(),
        username: "testuser".to_string(),
        name: name.to_string(),
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
        items: vec![],
    }
}

fn order_job(order_id: &str, name: &str) -> InvoiceJob {
    InvoiceJob::Order(Box::new(test_order(order_id, name)))
}

fn api_with_mock(mock_server: &MockServer) -> SevDeskApi {
    let mut api = SevDeskApi::new("test_token".to_string());
    api.base_url = mock_server.uri();
    api
}

/// Finalize and enshrine: the steps that must never run on a bad draft.
fn finalize_and_enshrine() -> InvoiceWorkflowOptions {
    InvoiceWorkflowOptions {
        finalize: true,
        enshrine: true,
        ..Default::default()
    }
}

/// Countries, contact, user, invoice creation (ID 500) and the workflow steps.
async fn mount_sevdesk(mock_server: &MockServer) {
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
    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{ "id": "10", "name": "Alice", "objectName": "Contact", "customerNumber": null, "status": null }]
        })))
        .mount(mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/SevUser"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{ "id": "1", "username": "admin", "objectName": "SevUser" }]
        })))
        .mount(mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/Invoice"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "objects": { "id": "500", "invoiceNumber": "RE-500" }
        })))
        .mount(mock_server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/Invoice/500/sendBy"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(mock_server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/Invoice/500/enshrine"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(mock_server)
        .await;
}

async fn mount_positions(mock_server: &MockServer, status: u16) {
    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(status).set_body_string("{}"))
        .mount(mock_server)
        .await;
}

async fn mount_total(mock_server: &MockServer, sum_gross: &str) {
    Mock::given(method("GET"))
        .and(path("/Invoice/500"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{ "id": "500", "sumGross": sum_gross }]
        })))
        .mount(mock_server)
        .await;
}

/// Waits for the next worker event.
async fn next_event(rx: &Receiver<ProgressEvent>) -> ProgressEvent {
    for _ in 0..1000 {
        match rx.try_recv() {
            Ok(event) => return event,
            Err(TryRecvError::Empty) => tokio::time::sleep(Duration::from_millis(5)).await,
            Err(TryRecvError::Disconnected) => panic!("worker finished without the event"),
        }
    }
    panic!("timed out waiting for a worker event");
}

/// A worker running in the background, driven by the test the way the GUI would.
struct Run {
    worker: tokio::task::JoinHandle<()>,
    rx: Receiver<ProgressEvent>,
    results: Vec<InvoiceCreationResult>,
}

impl Run {
    fn start(api: SevDeskApi, jobs: Vec<InvoiceJob>, options: InvoiceWorkflowOptions) -> Self {
        let (tx, rx) = mpsc::channel();
        let worker = tokio::spawn(run_invoice_jobs(api, jobs, false, options, tx));
        Self {
            worker,
            rx,
            results: Vec::new(),
        }
    }

    /// Waits for the next question; results reported before it are kept.
    async fn next_question(&mut self) -> DecisionRequest {
        loop {
            match next_event(&self.rx).await {
                ProgressEvent::DecisionNeeded(request) => return request,
                ProgressEvent::ItemFinished(result) => self.results.push(*result),
                _ => {}
            }
        }
    }

    async fn answer_invoice(&mut self, decision: InvoiceDecision) {
        match self.next_question().await {
            DecisionRequest::Invoice(_, reply) => reply.send(decision).unwrap(),
            other => panic!("unexpected question: {other:?}"),
        }
    }

    async fn answer_country(&mut self, decision: CountryDecision) {
        match self.next_question().await {
            DecisionRequest::Country(_, reply) => reply.send(decision).unwrap(),
            other => panic!("unexpected question: {other:?}"),
        }
    }

    /// Lets the worker finish and returns every result it reported.
    async fn finish(mut self) -> Vec<InvoiceCreationResult> {
        self.worker.await.unwrap();
        for event in self.rx.try_iter() {
            match event {
                ProgressEvent::DecisionNeeded(request) => {
                    panic!("unanswered question: {request:?}")
                }
                ProgressEvent::ItemFinished(result) => self.results.push(*result),
                _ => {}
            }
        }
        self.results
    }
}

async fn requests_to(mock_server: &MockServer, http_method: &str, suffix: &str) -> usize {
    mock_server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == http_method && r.url.path().ends_with(suffix))
        .count()
}

// ── basics ───────────────────────────────────────────────────────────

#[test]
fn job_label_contains_name_and_order_id() {
    assert_eq!(order_job("ORD-1", "Alice").label(), "Alice (ORD-1)");
}

#[tokio::test]
async fn worker_reports_every_item_and_closes_channel() {
    // Every API call fails, so each order must surface as an error result
    // without aborting the remaining orders.
    let mock_server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    let jobs = vec![order_job("ORD-1", "Alice"), order_job("ORD-2", "Bob")];
    let (tx, rx) = mpsc::channel();

    run_invoice_jobs(
        api_with_mock(&mock_server),
        jobs,
        false,
        InvoiceWorkflowOptions::default(),
        tx,
    )
    .await;

    let events: Vec<ProgressEvent> = rx.try_iter().collect();
    assert_eq!(events.len(), 4);
    assert!(matches!(&events[0], ProgressEvent::ItemStarted { label } if label == "Alice (ORD-1)"));
    assert!(
        matches!(&events[1], ProgressEvent::ItemFinished(r) if r.order_id == "ORD-1" && r.error.is_some())
    );
    assert!(matches!(&events[2], ProgressEvent::ItemStarted { label } if label == "Bob (ORD-2)"));
    assert!(
        matches!(&events[3], ProgressEvent::ItemFinished(r) if r.order_id == "ORD-2" && r.error.is_some())
    );
    // Sender was dropped with the worker: the GUI sees the run as done.
    assert!(matches!(
        rx.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[tokio::test]
async fn matching_total_runs_workflow_without_asking() {
    let mock_server = MockServer::start().await;
    mount_sevdesk(&mock_server).await;
    mount_positions(&mock_server, 201).await;
    mount_total(&mock_server, "10.00").await;

    let run = Run::start(
        api_with_mock(&mock_server),
        vec![order_job("ORD-1", "Alice")],
        finalize_and_enshrine(),
    );
    let results = run.finish().await;

    assert_eq!(results.len(), 1);
    assert!(results[0].error.is_none(), "{:?}", results[0].error);
    assert_eq!(results[0].invoice_number.as_deref(), Some("RE-500"));
    let status = results[0].workflow_status.as_ref().unwrap();
    assert!(status.finalized && status.enshrined);
}

// ── incomplete invoices ──────────────────────────────────────────────

#[tokio::test]
async fn rejected_line_item_pauses_and_is_never_finalized_or_enshrined() {
    let mock_server = MockServer::start().await;
    mount_sevdesk(&mock_server).await;
    mount_positions(&mock_server, 500).await;
    mount_total(&mock_server, "0.00").await;

    let mut run = Run::start(
        api_with_mock(&mock_server),
        vec![order_job("ORD-1", "Alice")],
        finalize_and_enshrine(),
    );

    // The question describes the problem.
    let question = match run.next_question().await {
        DecisionRequest::Invoice(question, reply) => {
            reply.send(InvoiceDecision::KeepDraft).unwrap();
            question
        }
        other => panic!("unexpected question: {other:?}"),
    };
    assert_eq!(question.invoice_number, "RE-500");
    assert_eq!(question.label, "Alice (ORD-1)");
    assert_eq!(question.problem.expected_cents, 1000);
    assert_eq!(question.problem.actual_cents, Ok(0));
    assert_eq!(question.problem.failed.len(), 1);

    let results = run.finish().await;
    let error = results[0].error.as_deref().unwrap();
    assert!(error.contains("Left as a draft"), "{error}");
    assert_eq!(results[0].invoice_id, Some(500));
    assert!(results[0].workflow_status.is_none());

    assert_eq!(requests_to(&mock_server, "PUT", "/sendBy").await, 0);
    assert_eq!(requests_to(&mock_server, "PUT", "/enshrine").await, 0);
}

#[tokio::test]
async fn retry_adds_the_failed_line_item_then_runs_workflow() {
    let mock_server = MockServer::start().await;
    mount_sevdesk(&mock_server).await;
    // First position attempt fails, the retry succeeds.
    Mock::given(method("POST"))
        .and(path("/InvoicePos"))
        .respond_with(ResponseTemplate::new(500).set_body_string("busy"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&mock_server)
        .await;
    mount_positions(&mock_server, 201).await;
    // Total is 0.00 before the retry, 10.00 after.
    Mock::given(method("GET"))
        .and(path("/Invoice/500"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{ "id": "500", "sumGross": "0.00" }]
        })))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&mock_server)
        .await;
    mount_total(&mock_server, "10.00").await;

    let mut run = Run::start(
        api_with_mock(&mock_server),
        vec![order_job("ORD-1", "Alice")],
        finalize_and_enshrine(),
    );
    run.answer_invoice(InvoiceDecision::RetryFailed).await;
    let results = run.finish().await;

    assert!(results[0].error.is_none(), "{:?}", results[0].error);
    assert!(results[0].workflow_status.as_ref().unwrap().enshrined);
    assert_eq!(requests_to(&mock_server, "POST", "/InvoicePos").await, 2);
}

#[tokio::test]
async fn check_again_continues_once_the_draft_was_fixed() {
    let mock_server = MockServer::start().await;
    mount_sevdesk(&mock_server).await;
    mount_positions(&mock_server, 500).await;
    // The user adds the missing item in SevDesk, then chooses "Check again".
    Mock::given(method("GET"))
        .and(path("/Invoice/500"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{ "id": "500", "sumGross": "0.00" }]
        })))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&mock_server)
        .await;
    mount_total(&mock_server, "10.00").await;

    let mut run = Run::start(
        api_with_mock(&mock_server),
        vec![order_job("ORD-1", "Alice")],
        finalize_and_enshrine(),
    );
    run.answer_invoice(InvoiceDecision::CheckAgain).await;
    let results = run.finish().await;

    assert!(results[0].error.is_none(), "{:?}", results[0].error);
    // Check again does not re-send line items.
    assert_eq!(requests_to(&mock_server, "POST", "/InvoicePos").await, 1);
}

#[tokio::test]
async fn still_wrong_after_retry_asks_again() {
    let mock_server = MockServer::start().await;
    mount_sevdesk(&mock_server).await;
    mount_positions(&mock_server, 500).await;
    mount_total(&mock_server, "0.00").await;

    let mut run = Run::start(
        api_with_mock(&mock_server),
        vec![order_job("ORD-1", "Alice")],
        finalize_and_enshrine(),
    );
    run.answer_invoice(InvoiceDecision::RetryFailed).await;
    run.answer_invoice(InvoiceDecision::KeepDraft).await;
    let results = run.finish().await;

    assert!(results[0].error.is_some());
    assert_eq!(requests_to(&mock_server, "PUT", "/sendBy").await, 0);
}

#[tokio::test]
async fn delete_draft_removes_it_and_continues_with_next_order() {
    let mock_server = MockServer::start().await;
    mount_sevdesk(&mock_server).await;
    mount_positions(&mock_server, 500).await;
    mount_total(&mock_server, "0.00").await;
    Mock::given(method("DELETE"))
        .and(path("/Invoice/500"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mut run = Run::start(
        api_with_mock(&mock_server),
        vec![order_job("ORD-1", "Alice"), order_job("ORD-2", "Bob")],
        InvoiceWorkflowOptions::default(),
    );
    run.answer_invoice(InvoiceDecision::DeleteDraft).await;
    run.answer_invoice(InvoiceDecision::KeepDraft).await;
    let results = run.finish().await;

    assert_eq!(results.len(), 2);
    let error = results[0].error.as_deref().unwrap();
    assert!(error.contains("was deleted"), "{error}");
    assert_eq!(results[0].invoice_id, None);
    // The second order was still processed (and asked about).
    assert_eq!(results[1].order_id, "ORD-2");
    assert!(results[1]
        .error
        .as_deref()
        .unwrap()
        .contains("Left as a draft"));
}

#[tokio::test]
async fn failed_delete_keeps_reporting_the_draft() {
    let mock_server = MockServer::start().await;
    mount_sevdesk(&mock_server).await;
    mount_positions(&mock_server, 500).await;
    mount_total(&mock_server, "0.00").await;
    Mock::given(method("DELETE"))
        .and(path("/Invoice/500"))
        .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
        .mount(&mock_server)
        .await;

    let mut run = Run::start(
        api_with_mock(&mock_server),
        vec![order_job("ORD-1", "Alice")],
        InvoiceWorkflowOptions::default(),
    );
    run.answer_invoice(InvoiceDecision::DeleteDraft).await;
    let results = run.finish().await;

    let error = results[0].error.as_deref().unwrap();
    assert!(error.contains("Deleting it failed"), "{error}");
    assert_eq!(results[0].invoice_id, Some(500));
}

#[tokio::test]
async fn stop_reports_remaining_orders_as_not_processed() {
    let mock_server = MockServer::start().await;
    mount_sevdesk(&mock_server).await;
    mount_positions(&mock_server, 500).await;
    mount_total(&mock_server, "0.00").await;

    let mut run = Run::start(
        api_with_mock(&mock_server),
        vec![
            order_job("ORD-1", "Alice"),
            order_job("ORD-2", "Bob"),
            order_job("ORD-3", "Carol"),
        ],
        finalize_and_enshrine(),
    );
    run.answer_invoice(InvoiceDecision::Stop).await;
    let results = run.finish().await;

    assert_eq!(results.len(), 3);
    assert!(results[0].error.as_deref().unwrap().contains("run stopped"));
    for result in &results[1..] {
        assert_eq!(
            result.error.as_deref(),
            Some("Not processed: run stopped by user")
        );
    }
    // Only the first order ever reached SevDesk.
    assert_eq!(requests_to(&mock_server, "POST", "/Invoice").await, 1);
}

#[tokio::test]
async fn unanswered_question_stops_safely() {
    let mock_server = MockServer::start().await;
    mount_sevdesk(&mock_server).await;
    mount_positions(&mock_server, 500).await;
    mount_total(&mock_server, "0.00").await;

    let mut run = Run::start(
        api_with_mock(&mock_server),
        vec![order_job("ORD-1", "Alice"), order_job("ORD-2", "Bob")],
        finalize_and_enshrine(),
    );
    // The GUI goes away without answering.
    drop(run.next_question().await);
    let results = run.finish().await;

    assert_eq!(results.len(), 2);
    assert!(results[0].error.as_deref().unwrap().contains("run stopped"));
    assert_eq!(requests_to(&mock_server, "PUT", "/enshrine").await, 0);
}

#[tokio::test]
async fn cardtrader_invoice_uses_the_same_check() {
    let mock_server = MockServer::start().await;
    mount_sevdesk(&mock_server).await;
    mount_positions(&mock_server, 500).await;
    mount_total(&mock_server, "0.00").await;

    let invoice = ConsolidatedInvoice {
        recipient: InvoiceRecipient {
            country: "Germany".to_string(),
            ..InvoiceRecipient::default()
        },
        invoice_date: "2026-07-05".to_string(),
        period_label: "2026-07-01 - 2026-07-05".to_string(),
        positions: vec![ConsolidatedPosition {
            item_name: "Carnage Tyrant".to_string(),
            item_expansion: "Ixalan".to_string(),
            item_properties: "Slightly Played - EN".to_string(),
            quantity: 1,
            total_cents: 342,
        }],
        currency: "EUR".to_string(),
        row_count: 1,
        skipped_row_count: 0,
    };
    // The recipient contact is looked up by exact name.
    Mock::given(method("GET"))
        .and(path("/Contact"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "objects": [{ "id": "11", "name": "GRAY FOX SRL", "objectName": "Contact", "customerNumber": null, "status": null }]
        })))
        .with_priority(1)
        .mount(&mock_server)
        .await;

    let mut run = Run::start(
        api_with_mock(&mock_server),
        vec![InvoiceJob::Consolidated(Box::new(invoice))],
        finalize_and_enshrine(),
    );
    run.answer_invoice(InvoiceDecision::KeepDraft).await;
    let results = run.finish().await;

    assert!(results[0]
        .error
        .as_deref()
        .unwrap()
        .contains("Left as a draft"));
    assert_eq!(requests_to(&mock_server, "PUT", "/enshrine").await, 0);
}

// ── unknown countries ────────────────────────────────────────────────

#[tokio::test]
async fn unknown_country_asks_and_uses_the_chosen_country() {
    let mock_server = MockServer::start().await;
    mount_sevdesk(&mock_server).await;
    mount_positions(&mock_server, 201).await;
    mount_total(&mock_server, "10.00").await;

    let mut first = test_order("ORD-1", "Alice");
    first.country = "Niger".to_string();
    let mut second = test_order("ORD-2", "Bob");
    second.country = "niger ".to_string();

    let mut run = Run::start(
        api_with_mock(&mock_server),
        vec![
            InvoiceJob::Order(Box::new(first)),
            InvoiceJob::Order(Box::new(second)),
        ],
        InvoiceWorkflowOptions::default(),
    );

    match run.next_question().await {
        DecisionRequest::Country(question, reply) => {
            assert_eq!(question.country, "Niger");
            // Not silently matched to Nigeria: the user picks.
            assert!(question.options.iter().any(|o| o.id == 50));
            reply.send(CountryDecision::Use(1)).unwrap();
        }
        other => panic!("unexpected question: {other:?}"),
    }
    // The second order has the same country and is not asked about again.
    let results = run.finish().await;
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.error.is_none()), "{results:?}");

    let requests = mock_server.received_requests().await.unwrap();
    let invoice_bodies: Vec<serde_json::Value> = requests
        .iter()
        .filter(|r| r.method.as_str() == "POST" && r.url.path() == "/Invoice")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect();
    assert_eq!(invoice_bodies.len(), 2);
    for body in invoice_bodies {
        assert_eq!(body["addressCountry"]["id"], 1);
    }
}

#[tokio::test]
async fn unknown_country_skip_creates_nothing() {
    let mock_server = MockServer::start().await;
    mount_sevdesk(&mock_server).await;

    let mut order = test_order("ORD-1", "Alice");
    order.country = "Atlantis".to_string();

    let mut run = Run::start(
        api_with_mock(&mock_server),
        vec![InvoiceJob::Order(Box::new(order))],
        finalize_and_enshrine(),
    );
    run.answer_country(CountryDecision::Skip).await;
    let results = run.finish().await;

    let error = results[0].error.as_deref().unwrap();
    assert!(error.contains("Unknown country 'Atlantis'"), "{error}");
    let writes = mock_server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() != "GET")
        .count();
    assert_eq!(writes, 0);
}

// ── dry run ──────────────────────────────────────────────────────────

#[tokio::test]
async fn dry_run_reports_problems_without_asking() {
    let mock_server = MockServer::start().await;
    mount_sevdesk(&mock_server).await;

    let mut wrong_total = test_order("ORD-1", "Alice");
    wrong_total.total_value = "12,00".to_string();
    let mut unknown_country = test_order("ORD-2", "Bob");
    unknown_country.country = "Atlantis".to_string();

    let (tx, rx) = mpsc::channel();
    run_invoice_jobs(
        api_with_mock(&mock_server),
        vec![
            InvoiceJob::Order(Box::new(wrong_total)),
            InvoiceJob::Order(Box::new(unknown_country)),
            order_job("ORD-3", "Carol"),
        ],
        true,
        finalize_and_enshrine(),
        tx,
    )
    .await;

    let results: Vec<InvoiceCreationResult> = rx
        .try_iter()
        .filter_map(|event| match event {
            ProgressEvent::DecisionNeeded(request) => panic!("dry run asked: {request:?}"),
            ProgressEvent::ItemFinished(result) => Some(*result),
            _ => None,
        })
        .collect();

    assert_eq!(
        results[0].error.as_deref(),
        Some("Line items add up to 10.00 EUR but the order total is 12.00 EUR")
    );
    assert!(results[1]
        .error
        .as_deref()
        .unwrap()
        .contains("A real run will ask you"));
    assert!(results[2].error.is_none(), "{:?}", results[2].error);
}
