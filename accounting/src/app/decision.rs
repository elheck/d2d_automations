//! Questions the invoice worker asks the user, and the user's answers.
//!
//! When an invoice cannot safely continue, the worker sends a
//! [`DecisionRequest`] and waits. The GUI shows it, and the answer travels back
//! over the request's own one-shot channel. Nothing is finalized, enshrined or
//! booked while a question is open. If the GUI goes away without answering,
//! the worker stops and leaves any draft untouched.

use std::sync::mpsc::Sender;

use tokio::sync::oneshot;

use crate::sevdesk_api::{CountryOption, DraftProblem};

use super::progress::ProgressEvent;

/// An order's country is not a SevDesk country name.
#[derive(Debug, Clone, PartialEq)]
pub struct CountryQuestion {
    /// Which order this is about, e.g. `Alice (1234)`.
    pub label: String,
    /// The country name from the CSV.
    pub country: String,
    /// All SevDesk countries to choose from.
    pub options: Vec<CountryOption>,
}

/// Answer to a [`CountryQuestion`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountryDecision {
    /// Use this SevDesk country ID for every order with this country name in
    /// the current run.
    Use(u32),
    /// Do not invoice this order; continue with the next one.
    Skip,
    /// Do not invoice this or any remaining order.
    Stop,
}

/// A draft invoice does not add up to what it should.
#[derive(Debug, Clone, PartialEq)]
pub struct InvoiceQuestion {
    /// Which order this is about, e.g. `Alice (1234)`.
    pub label: String,
    pub invoice_number: String,
    pub currency: String,
    pub problem: DraftProblem,
}

/// Answer to an [`InvoiceQuestion`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvoiceDecision {
    /// Add the rejected line items again, then re-check the total.
    RetryFailed,
    /// Re-check the total, e.g. after fixing the draft in SevDesk by hand.
    CheckAgain,
    /// Leave the draft in SevDesk unfinished and continue with the next order.
    KeepDraft,
    /// Delete the draft from SevDesk and continue with the next order.
    DeleteDraft,
    /// Leave the draft and do not process the remaining orders.
    Stop,
}

/// A question waiting for the user, with the channel its answer goes back on.
#[derive(Debug)]
pub enum DecisionRequest {
    Country(CountryQuestion, oneshot::Sender<CountryDecision>),
    Invoice(InvoiceQuestion, oneshot::Sender<InvoiceDecision>),
}

/// Sends a question to the GUI and waits for the answer.
///
/// Returns `on_gone` if the GUI is gone or drops the question unanswered.
async fn ask<T>(
    tx: &Sender<ProgressEvent>,
    make_request: impl FnOnce(oneshot::Sender<T>) -> DecisionRequest,
    on_gone: T,
) -> T {
    let (reply_tx, reply_rx) = oneshot::channel();
    if tx
        .send(ProgressEvent::DecisionNeeded(make_request(reply_tx)))
        .is_err()
    {
        return on_gone;
    }
    reply_rx.await.unwrap_or(on_gone)
}

/// Asks which SevDesk country an unknown country name means.
pub(super) async fn ask_country(
    tx: &Sender<ProgressEvent>,
    question: CountryQuestion,
) -> CountryDecision {
    ask(
        tx,
        |reply| DecisionRequest::Country(question, reply),
        CountryDecision::Stop,
    )
    .await
}

/// Asks what to do with a draft invoice that does not add up.
pub(super) async fn ask_invoice(
    tx: &Sender<ProgressEvent>,
    question: InvoiceQuestion,
) -> InvoiceDecision {
    ask(
        tx,
        |reply| DecisionRequest::Invoice(question, reply),
        InvoiceDecision::Stop,
    )
    .await
}

#[cfg(test)]
#[path = "decision_tests.rs"]
mod tests;
