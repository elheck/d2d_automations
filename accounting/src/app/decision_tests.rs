//! Tests for the worker ↔ GUI question protocol.

use std::sync::mpsc;

use super::*;
use crate::sevdesk_api::{CountryOption, DraftProblem};

fn country_question() -> CountryQuestion {
    CountryQuestion {
        label: "Alice (ORD-1)".to_string(),
        country: "Bundesrepublik".to_string(),
        options: vec![CountryOption {
            id: 1,
            display_name: "Germany (Deutschland)".to_string(),
        }],
    }
}

fn invoice_question() -> InvoiceQuestion {
    InvoiceQuestion {
        label: "Alice (ORD-1)".to_string(),
        invoice_number: "RE-1".to_string(),
        currency: "EUR".to_string(),
        problem: DraftProblem {
            expected_cents: 1000,
            planned_cents: 1000,
            actual_cents: Ok(0),
            failed: vec![],
        },
    }
}

#[tokio::test]
async fn country_answer_travels_back_to_the_worker() {
    let (tx, rx) = mpsc::channel();
    let worker = tokio::spawn(async move { ask_country(&tx, country_question()).await });

    let event = tokio::task::spawn_blocking(move || rx.recv().unwrap())
        .await
        .unwrap();
    match event {
        ProgressEvent::DecisionNeeded(DecisionRequest::Country(question, reply)) => {
            assert_eq!(question, country_question());
            reply.send(CountryDecision::Use(1)).unwrap();
        }
        other => panic!("unexpected event: {other:?}"),
    }

    assert_eq!(worker.await.unwrap(), CountryDecision::Use(1));
}

#[tokio::test]
async fn invoice_answer_travels_back_to_the_worker() {
    let (tx, rx) = mpsc::channel();
    let worker = tokio::spawn(async move { ask_invoice(&tx, invoice_question()).await });

    let event = tokio::task::spawn_blocking(move || rx.recv().unwrap())
        .await
        .unwrap();
    match event {
        ProgressEvent::DecisionNeeded(DecisionRequest::Invoice(question, reply)) => {
            assert_eq!(question, invoice_question());
            reply.send(InvoiceDecision::RetryFailed).unwrap();
        }
        other => panic!("unexpected event: {other:?}"),
    }

    assert_eq!(worker.await.unwrap(), InvoiceDecision::RetryFailed);
}

#[tokio::test]
async fn closed_gui_means_stop() {
    let (tx, rx) = mpsc::channel();
    drop(rx);
    assert_eq!(
        ask_country(&tx, country_question()).await,
        CountryDecision::Stop
    );
    assert_eq!(
        ask_invoice(&tx, invoice_question()).await,
        InvoiceDecision::Stop
    );
}

#[tokio::test]
async fn unanswered_question_means_stop() {
    let (tx, rx) = mpsc::channel();
    let worker = tokio::spawn(async move { ask_invoice(&tx, invoice_question()).await });

    // The GUI receives the question but drops it without answering.
    let event = tokio::task::spawn_blocking(move || rx.recv().unwrap())
        .await
        .unwrap();
    drop(event);

    assert_eq!(worker.await.unwrap(), InvoiceDecision::Stop);
}
