//! Background invoice worker.
//!
//! Runs on the Tokio runtime, one job after another, and reports progress to
//! the GUI. Each real (non-dry-run) invoice goes through these steps:
//!
//! 1. The country must be a SevDesk country name; otherwise the user picks one.
//! 2. A draft invoice is created and its line items are added.
//! 3. SevDesk's total must equal the expected total; otherwise the run pauses
//!    and the user decides what happens to the draft.
//! 4. Only then do finalize / enshrine / book run.

use std::sync::mpsc::Sender;

use log::{debug, error, info, warn};

use crate::models::{
    ConsolidatedInvoice, InvoiceCreationResult, InvoiceWorkflowOptions, OrderRecord,
};
use crate::sevdesk_api::{InvoiceDraft, SevDeskApi};

use super::decision::{
    ask_country, ask_invoice, CountryDecision, CountryQuestion, InvoiceDecision, InvoiceQuestion,
};
use super::progress::ProgressEvent;

/// One unit of background work: a single invoice to create.
#[derive(Debug, Clone)]
pub(super) enum InvoiceJob {
    Order(Box<OrderRecord>),
    Consolidated(Box<ConsolidatedInvoice>),
}

impl InvoiceJob {
    fn order_id(&self) -> String {
        match self {
            InvoiceJob::Order(order) => order.order_id.clone(),
            InvoiceJob::Consolidated(invoice) => format!("CardTrader {}", invoice.period_label),
        }
    }

    fn customer_name(&self) -> &str {
        match self {
            InvoiceJob::Order(order) => &order.name,
            InvoiceJob::Consolidated(invoice) => &invoice.recipient.name,
        }
    }

    /// Date used as payment date when booking the invoice.
    fn payment_date(&self) -> &str {
        match self {
            InvoiceJob::Order(order) => &order.date_of_purchase,
            InvoiceJob::Consolidated(invoice) => &invoice.invoice_date,
        }
    }

    /// Country of the invoice address, as written in the CSV or recipient form.
    fn country(&self) -> &str {
        match self {
            InvoiceJob::Order(order) => &order.country,
            InvoiceJob::Consolidated(invoice) => &invoice.recipient.country,
        }
    }

    fn currency(&self) -> &str {
        match self {
            InvoiceJob::Order(order) => &order.currency,
            InvoiceJob::Consolidated(invoice) => &invoice.currency,
        }
    }

    /// Human-readable identification shown in the progress display.
    pub(super) fn label(&self) -> String {
        format!("{} ({})", self.customer_name(), self.order_id())
    }

    /// A result for this job that has no invoice in SevDesk.
    fn failed(&self, error: impl Into<String>) -> InvoiceCreationResult {
        InvoiceCreationResult {
            order_id: self.order_id(),
            customer_name: self.customer_name().to_string(),
            invoice_id: None,
            invoice_number: None,
            error: Some(error.into()),
            workflow_status: None,
        }
    }

    /// A result for this job's draft; `error` is set when the draft is unfinished.
    fn with_draft(&self, draft: &InvoiceDraft, error: Option<String>) -> InvoiceCreationResult {
        InvoiceCreationResult {
            order_id: self.order_id(),
            customer_name: self.customer_name().to_string(),
            invoice_id: Some(draft.invoice_id),
            invoice_number: Some(draft.invoice_number.clone()),
            error,
            workflow_status: None,
        }
    }

    async fn create_draft(&self, api: &SevDeskApi) -> anyhow::Result<InvoiceDraft> {
        match self {
            InvoiceJob::Order(order) => api.create_order_draft(order).await,
            InvoiceJob::Consolidated(invoice) => api.create_consolidated_draft(invoice).await,
        }
    }

    async fn simulate(&self, api: &SevDeskApi) -> InvoiceCreationResult {
        let result = match self {
            InvoiceJob::Order(order) => api.simulate_invoice_creation(order).await,
            InvoiceJob::Consolidated(invoice) => api.simulate_consolidated_invoice(invoice).await,
        };
        result.unwrap_or_else(|e| self.failed(format!("{e:#}")))
    }
}

/// What happened to one job, and whether the run must stop after it.
struct JobOutcome {
    result: InvoiceCreationResult,
    stop: bool,
}

impl JobOutcome {
    fn next(result: InvoiceCreationResult) -> Self {
        Self {
            result,
            stop: false,
        }
    }

    fn stop(result: InvoiceCreationResult) -> Self {
        Self { result, stop: true }
    }
}

/// Processes all jobs sequentially, reporting progress over `tx`.
///
/// Pauses whenever a question must be answered. If the user stops the run,
/// every remaining job is reported as not processed. Send errors are ignored:
/// they only mean the GUI went away.
pub(super) async fn run_invoice_jobs(
    api: SevDeskApi,
    jobs: Vec<InvoiceJob>,
    dry_run: bool,
    workflow_options: InvoiceWorkflowOptions,
    tx: Sender<ProgressEvent>,
) {
    let total = jobs.len();
    let mut success_count = 0;
    let verb = if dry_run { "simulate" } else { "create" };

    for (index, job) in jobs.iter().enumerate() {
        let label = job.label();
        debug!("Job {}/{}: {} ({verb})", index + 1, total, label);
        let _ = tx.send(ProgressEvent::ItemStarted {
            label: label.clone(),
        });

        let outcome = process_job(&api, job, dry_run, &workflow_options, &tx).await;
        match &outcome.result.error {
            None => {
                success_count += 1;
                info!(
                    "Invoice for {label}: {}",
                    outcome.result.invoice_number.as_deref().unwrap_or("?")
                );
            }
            Some(err) => error!("Failed to {verb} invoice for {label}: {err}"),
        }
        let _ = tx.send(ProgressEvent::ItemFinished(Box::new(outcome.result)));

        if outcome.stop {
            let remaining = &jobs[index + 1..];
            warn!(
                "Run stopped by user; {} order(s) not processed",
                remaining.len()
            );
            for job in remaining {
                let _ = tx.send(ProgressEvent::ItemFinished(Box::new(
                    job.failed("Not processed: run stopped by user"),
                )));
            }
            break;
        }
    }

    info!(
        "Invoice {} completed: {} successful, {} errors",
        if dry_run { "simulation" } else { "processing" },
        success_count,
        total - success_count
    );
}

async fn process_job(
    api: &SevDeskApi,
    job: &InvoiceJob,
    dry_run: bool,
    options: &InvoiceWorkflowOptions,
    tx: &Sender<ProgressEvent>,
) -> JobOutcome {
    if dry_run {
        // The simulation reports problems as errors instead of asking.
        let result = job.simulate(api).await;
        return JobOutcome::next(run_workflow(api, job, result, options, true, tx).await);
    }

    if let Err(outcome) = ensure_country(api, job, tx).await {
        return outcome;
    }

    let mut draft = match job.create_draft(api).await {
        Ok(draft) => draft,
        Err(e) => return JobOutcome::next(job.failed(format!("{e:#}"))),
    };

    if let Some(outcome) = resolve_draft(api, job, &mut draft, tx).await {
        return outcome;
    }

    let result = job.with_draft(&draft, None);
    JobOutcome::next(run_workflow(api, job, result, options, false, tx).await)
}

/// Makes sure the job's country is a SevDesk country, asking the user if not.
async fn ensure_country(
    api: &SevDeskApi,
    job: &InvoiceJob,
    tx: &Sender<ProgressEvent>,
) -> Result<(), JobOutcome> {
    let country = job.country();
    let not_loaded =
        |e: anyhow::Error| JobOutcome::next(job.failed(format!("Could not load countries: {e:#}")));

    if api
        .lookup_country_id(country)
        .await
        .map_err(not_loaded)?
        .is_some()
    {
        return Ok(());
    }

    warn!("Unknown country '{country}' for {}", job.label());
    let question = CountryQuestion {
        label: job.label(),
        country: country.to_string(),
        options: api.country_options().await.map_err(not_loaded)?,
    };

    match ask_country(tx, question).await {
        CountryDecision::Use(id) => {
            api.remember_country(country, id).await;
            Ok(())
        }
        CountryDecision::Skip => Err(JobOutcome::next(job.failed(format!(
            "Unknown country '{country}': order skipped, nothing was created"
        )))),
        CountryDecision::Stop => Err(JobOutcome::stop(job.failed(format!(
            "Unknown country '{country}': run stopped, nothing was created for this order"
        )))),
    }
}

/// Holds a draft back until SevDesk's total matches, or the user decides
/// otherwise.
///
/// Returns `None` when the draft may continue to the workflow, or the outcome
/// the user chose for it.
async fn resolve_draft(
    api: &SevDeskApi,
    job: &InvoiceJob,
    draft: &mut InvoiceDraft,
    tx: &Sender<ProgressEvent>,
) -> Option<JobOutcome> {
    loop {
        // No problem means the total matches: return `None` and continue.
        let problem = api.verify_draft(draft).await?;
        let summary = problem.summary();
        let number = draft.invoice_number.clone();
        let question = InvoiceQuestion {
            label: job.label(),
            invoice_number: number.clone(),
            currency: job.currency().to_string(),
            problem,
        };

        match ask_invoice(tx, question).await {
            InvoiceDecision::RetryFailed => {
                let positions: Vec<_> = std::mem::take(&mut draft.failed)
                    .into_iter()
                    .map(|failed| failed.position)
                    .collect();
                info!(
                    "Retrying {} line item(s) on invoice #{number}",
                    positions.len()
                );
                draft.failed = api.add_positions(draft.invoice_id, &positions).await;
            }
            InvoiceDecision::CheckAgain => info!("Checking invoice #{number} again"),
            InvoiceDecision::KeepDraft => {
                return Some(JobOutcome::next(job.with_draft(
                    draft,
                    Some(format!(
                        "Incomplete invoice #{number}: {summary}. Left as a draft in SevDesk \
                         (not finalized, enshrined or booked)."
                    )),
                )));
            }
            InvoiceDecision::DeleteDraft => {
                let result = match api.delete_invoice(draft.invoice_id).await {
                    Ok(()) => job.failed(format!(
                        "Incomplete invoice #{number} ({summary}) was deleted. \
                         This order is not invoiced."
                    )),
                    Err(e) => job.with_draft(
                        draft,
                        Some(format!(
                            "Incomplete invoice #{number} ({summary}). Deleting it failed \
                             ({e:#}); it is still a draft in SevDesk."
                        )),
                    ),
                };
                return Some(JobOutcome::next(result));
            }
            InvoiceDecision::Stop => {
                return Some(JobOutcome::stop(job.with_draft(
                    draft,
                    Some(format!(
                        "Incomplete invoice #{number}: {summary}. Left as a draft in SevDesk; \
                         run stopped."
                    )),
                )));
            }
        }
    }
}

/// Runs finalize / enshrine / book for a finished invoice, if any are enabled.
async fn run_workflow(
    api: &SevDeskApi,
    job: &InvoiceJob,
    mut result: InvoiceCreationResult,
    options: &InvoiceWorkflowOptions,
    dry_run: bool,
    tx: &Sender<ProgressEvent>,
) -> InvoiceCreationResult {
    let wants_workflow = options.finalize || options.enshrine || options.book;
    let (Some(invoice_id), None, true) = (result.invoice_id, &result.error, wants_workflow) else {
        return result;
    };

    let _ = tx.send(ProgressEvent::WorkflowStarted);

    let mut options = options.clone();
    options.payment_date = Some(job.payment_date().to_string());
    let invoice_number = result
        .invoice_number
        .clone()
        .unwrap_or_else(|| "Unknown".to_string());

    let status = if dry_run {
        api.simulate_invoice_workflow(invoice_id, &invoice_number, &options)
            .await
    } else {
        api.execute_invoice_workflow(invoice_id, &invoice_number, &options)
            .await
    };

    if let Some(ref err) = status.workflow_error {
        error!("Workflow error for {}: {err}", job.label());
    }
    result.workflow_status = Some(status);
    result
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
