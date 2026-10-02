//! Business logic for the InvoiceApp (API calls, CSV loading, invoice processing).

use std::sync::mpsc::{self, Sender};

use log::{debug, error, info, warn};

use crate::{
    csv_processor::{cardtrader_parser, CsvProcessor, LoadedCsv},
    models::{
        CheckAccountResponse, ConsolidatedInvoice, InvoiceCreationResult, InvoiceWorkflowOptions,
        OrderRecord,
    },
    sevdesk_api::SevDeskApi,
};

use super::progress::{ProgressEvent, ProgressTracker};
use super::{InvoiceApp, ProcessingState};

impl InvoiceApp {
    pub(super) fn test_api_connection(&mut self) {
        debug!(
            "Testing API connection with token length: {}",
            self.api_token.len()
        );
        if !self.api_token.is_empty() {
            let api = SevDeskApi::new(self.api_token.clone());
            match self.runtime.block_on(api.test_connection()) {
                Ok(success) => {
                    if success {
                        info!("API connection test successful");
                        // Automatically load check accounts on successful connection
                        self.load_check_accounts();
                    } else {
                        warn!("API connection test failed");
                    }
                    self.api_connection_status = Some(success);
                }
                Err(e) => {
                    error!("API connection test error: {e}");
                    self.api_connection_status = Some(false);
                }
            }
        } else {
            warn!("Attempted to test API connection with empty token");
        }
    }

    pub(super) fn load_check_accounts(&mut self) {
        info!("Loading check accounts");
        self.check_accounts_loading = true;
        self.check_accounts_error = None;

        let api = SevDeskApi::new(self.api_token.clone());
        match self.runtime.block_on(api.fetch_check_accounts()) {
            Ok(accounts) => {
                info!("Loaded {} check accounts", accounts.len());

                // Find and auto-select the default account
                let default_index = accounts.iter().position(|a| a.is_default());
                if let Some(idx) = default_index {
                    info!("Auto-selecting default account: {}", accounts[idx].name);
                    self.selected_check_account_index = Some(idx);
                }

                self.check_accounts = accounts;
                self.check_accounts_loading = false;
            }
            Err(e) => {
                error!("Failed to load check accounts: {e}");
                self.check_accounts_error = Some(e.to_string());
                self.check_accounts_loading = false;
            }
        }
    }

    /// Returns the currently selected check account, if any.
    #[allow(dead_code)]
    pub fn selected_check_account(&self) -> Option<&CheckAccountResponse> {
        self.selected_check_account_index
            .and_then(|idx| self.check_accounts.get(idx))
    }

    pub(super) fn load_csv_file(&mut self) {
        debug!("Opening file dialog for CSV selection");
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("CSV files", &["csv"])
            .pick_file()
        {
            info!("Selected CSV file: {path:?}");
            self.processing_state = ProcessingState::LoadingCsv;
            self.csv_file_path = Some(path.clone());

            let processor = CsvProcessor::new();
            debug!("Starting CSV file processing");

            // Reset both paths so a newly loaded file never mixes with the previous one.
            self.orders.clear();
            self.cardtrader_rows.clear();
            self.consolidated_invoice = None;

            match self.runtime.block_on(processor.load_csv(&path)) {
                Ok(LoadedCsv::Cardmarket(orders)) => {
                    info!("Successfully loaded {} orders from CSV", orders.len());
                    // Validate orders
                    debug!("Validating loaded orders");
                    self.validation_errors = processor.validate_orders(&orders);

                    if self.validation_errors.is_empty() {
                        info!("All orders passed validation");
                        self.orders = orders;
                    } else {
                        warn!("Found {} validation errors", self.validation_errors.len());
                        for error in &self.validation_errors {
                            warn!("Validation error: {error}");
                        }
                        self.orders.clear();
                    }

                    self.processing_state = ProcessingState::Idle;
                }
                Ok(LoadedCsv::CardTrader(rows)) => {
                    info!("Loaded {} rows from CardTrader sales report", rows.len());
                    self.cardtrader_rows = rows;
                    self.rebuild_consolidated_invoice();
                    self.processing_state = ProcessingState::Idle;
                }
                Err(e) => {
                    error!("Failed to load CSV file: {e}");
                    self.validation_errors = vec![format!("Failed to load CSV file: {}", e)];
                    self.orders.clear();
                    self.cardtrader_rows.clear();
                    self.consolidated_invoice = None;
                    self.processing_state = ProcessingState::Idle;
                }
            }
        } else {
            debug!("File dialog cancelled by user");
        }
    }

    /// Recomputes the consolidated invoice from the loaded CardTrader rows and
    /// the current recipient, refreshing validation errors.
    ///
    /// Called after loading a report and whenever the recipient is edited.
    pub(super) fn rebuild_consolidated_invoice(&mut self) {
        if self.cardtrader_rows.is_empty() {
            self.consolidated_invoice = None;
            return;
        }

        match cardtrader_parser::consolidate(
            &self.cardtrader_rows,
            self.cardtrader_recipient.clone(),
        ) {
            Ok(invoice) => {
                self.validation_errors = cardtrader_parser::validate_consolidated(&invoice);
                if self.validation_errors.is_empty() {
                    info!(
                        "Consolidated invoice: {} positions, net {:.2} {}",
                        invoice.positions.len(),
                        invoice.total(),
                        invoice.currency
                    );
                } else {
                    for error in &self.validation_errors {
                        warn!("Consolidated invoice validation error: {error}");
                    }
                }
                self.consolidated_invoice = Some(invoice);
            }
            Err(e) => {
                error!("Failed to consolidate CardTrader report: {e}");
                self.validation_errors = vec![format!("Failed to consolidate report: {}", e)];
                self.consolidated_invoice = None;
            }
        }
    }

    /// Starts creating the single consolidated invoice for a loaded CardTrader report.
    pub(super) fn process_consolidated_invoice(&mut self) {
        let Some(invoice) = self.consolidated_invoice.clone() else {
            warn!("Cannot process consolidated invoice: nothing consolidated");
            return;
        };

        if self.api_token.is_empty() {
            warn!("Cannot process consolidated invoice: API token is empty");
            return;
        }

        let validation_errors = cardtrader_parser::validate_consolidated(&invoice);
        if !validation_errors.is_empty() {
            warn!("Refusing to invoice: consolidated invoice is invalid");
            self.validation_errors = validation_errors;
            return;
        }

        self.start_invoice_jobs(vec![InvoiceJob::Consolidated(Box::new(invoice))]);
    }

    /// Starts creating one invoice per loaded order.
    pub(super) fn process_invoices(&mut self) {
        info!(
            "Starting invoice {} for {} orders",
            if self.dry_run_mode {
                "simulation"
            } else {
                "processing"
            },
            self.orders.len()
        );
        if self.orders.is_empty() || self.api_token.is_empty() {
            warn!(
                "Cannot process invoices: orders={}, token_empty={}",
                self.orders.len(),
                self.api_token.is_empty()
            );
            return;
        }

        let jobs = self
            .orders
            .iter()
            .cloned()
            .map(|order| InvoiceJob::Order(Box::new(order)))
            .collect();
        self.start_invoice_jobs(jobs);
    }

    /// Spawns the jobs on the runtime so the GUI keeps rendering progress.
    fn start_invoice_jobs(&mut self, jobs: Vec<InvoiceJob>) {
        if self.progress.is_some() {
            warn!("Invoice processing already running");
            return;
        }

        let total = jobs.len();
        let (tx, rx) = mpsc::channel();

        self.results.clear();
        self.last_run_duration = None;
        self.processing_state = ProcessingState::Processing { current: 0, total };
        self.progress = Some(ProgressTracker::new(rx, total, self.dry_run_mode));

        let api = SevDeskApi::new(self.api_token.clone());
        self.runtime.spawn(run_invoice_jobs(
            api,
            jobs,
            self.dry_run_mode,
            self.build_workflow_options(),
            tx,
        ));
    }

    /// Folds pending worker events into the app state. Called every frame.
    pub(super) fn poll_progress(&mut self) {
        let Some(tracker) = self.progress.as_mut() else {
            return;
        };

        let poll = tracker.poll();
        self.results.extend(poll.results);
        self.processing_state = ProcessingState::Processing {
            current: tracker.completed,
            total: tracker.total,
        };

        if poll.done {
            self.last_run_duration = Some(tracker.elapsed());
            self.progress = None;
            self.processing_state = ProcessingState::Completed;
        }
    }

    /// Builds workflow options from current UI state
    fn build_workflow_options(&self) -> InvoiceWorkflowOptions {
        InvoiceWorkflowOptions {
            finalize: self.workflow_finalize,
            send_type: self.workflow_send_type.clone(),
            enshrine: self.workflow_enshrine,
            book: self.workflow_book,
            check_account_id: self
                .selected_check_account_index
                .and_then(|idx| self.check_accounts.get(idx))
                .map(|acc| acc.id.clone()),
            pdf_download_path: self.pdf_download_path.clone(),
            payment_date: None, // Will be set per-order
        }
    }
}

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

    /// Human-readable identification shown in the progress display.
    pub(super) fn label(&self) -> String {
        format!("{} ({})", self.customer_name(), self.order_id())
    }
}

/// Processes all jobs sequentially, reporting progress over `tx`.
///
/// Send errors are ignored: they only mean the GUI went away.
pub(super) async fn run_invoice_jobs(
    api: SevDeskApi,
    jobs: Vec<InvoiceJob>,
    dry_run: bool,
    workflow_options: InvoiceWorkflowOptions,
    tx: Sender<ProgressEvent>,
) {
    let total = jobs.len();
    let mut success_count = 0;

    for (index, job) in jobs.iter().enumerate() {
        let label = job.label();
        debug!(
            "{} {}/{}: {}",
            if dry_run { "Simulating" } else { "Processing" },
            index + 1,
            total,
            label
        );
        let _ = tx.send(ProgressEvent::ItemStarted {
            label: label.clone(),
        });

        let result = match (job, dry_run) {
            (InvoiceJob::Order(order), true) => api.simulate_invoice_creation(order).await,
            (InvoiceJob::Order(order), false) => api.create_invoice(order).await,
            (InvoiceJob::Consolidated(invoice), true) => {
                api.simulate_consolidated_invoice(invoice).await
            }
            (InvoiceJob::Consolidated(invoice), false) => {
                api.create_consolidated_invoice(invoice).await
            }
        };

        let final_result = match result {
            Ok(mut invoice_result) => {
                if let Some(ref err) = invoice_result.error {
                    error!(
                        "Failed to {} invoice for {}: {}",
                        if dry_run { "simulate" } else { "create" },
                        label,
                        err
                    );
                } else {
                    info!(
                        "{} invoice for {}: {}",
                        if dry_run {
                            "Simulated"
                        } else {
                            "Successfully created"
                        },
                        label,
                        invoice_result
                            .invoice_number
                            .as_deref()
                            .unwrap_or("[DRY RUN]")
                    );
                }

                // Execute workflow if invoice was created successfully
                let wants_workflow =
                    workflow_options.finalize || workflow_options.enshrine || workflow_options.book;
                if let (Some(invoice_id), None, true) = (
                    invoice_result.invoice_id,
                    &invoice_result.error,
                    wants_workflow,
                ) {
                    let _ = tx.send(ProgressEvent::WorkflowStarted);

                    let mut options = workflow_options.clone();
                    options.payment_date = Some(job.payment_date().to_string());
                    let invoice_number = invoice_result
                        .invoice_number
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string());

                    let workflow_status = if dry_run {
                        api.simulate_invoice_workflow(invoice_id, &invoice_number, &options)
                            .await
                    } else {
                        api.execute_invoice_workflow(invoice_id, &invoice_number, &options)
                            .await
                    };

                    if let Some(ref err) = workflow_status.workflow_error {
                        error!("Workflow error for {label}: {err}");
                    }

                    invoice_result.workflow_status = Some(workflow_status);
                }

                invoice_result
            }
            Err(e) => {
                error!(
                    "Error {} invoice for {}: {}",
                    if dry_run { "simulating" } else { "processing" },
                    label,
                    e
                );
                InvoiceCreationResult {
                    order_id: job.order_id(),
                    customer_name: job.customer_name().to_string(),
                    invoice_id: None,
                    invoice_number: None,
                    error: Some(e.to_string()),
                    workflow_status: None,
                }
            }
        };

        if final_result.error.is_none() {
            success_count += 1;
        }
        let _ = tx.send(ProgressEvent::ItemFinished(Box::new(final_result)));
    }

    info!(
        "Invoice {} completed: {} successful, {} errors",
        if dry_run { "simulation" } else { "processing" },
        success_count,
        total - success_count
    );
}
