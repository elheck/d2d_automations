//! Business logic for the InvoiceApp (API calls, CSV loading, invoice processing).

use log::{debug, error, info, warn};

use crate::{
    csv_processor::CsvProcessor,
    models::{CheckAccountResponse, InvoiceCreationResult, InvoiceWorkflowOptions},
    sevdesk_api::SevDeskApi,
};

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
            match self.runtime.block_on(processor.load_orders_from_csv(&path)) {
                Ok(orders) => {
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
                Err(e) => {
                    error!("Failed to load CSV file: {e}");
                    self.validation_errors = vec![format!("Failed to load CSV file: {}", e)];
                    self.orders.clear();
                    self.processing_state = ProcessingState::Idle;
                }
            }
        } else {
            debug!("File dialog cancelled by user");
        }
    }

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

        self.results.clear();
        self.processing_state = ProcessingState::Processing {
            current: 0,
            total: self.orders.len(),
        };

        let api = SevDeskApi::new(self.api_token.clone());

        for (index, order) in self.orders.iter().enumerate() {
            let action = if self.dry_run_mode {
                "Simulating"
            } else {
                "Processing"
            };
            debug!(
                "{} order {}/{}: {} ({})",
                action,
                index + 1,
                self.orders.len(),
                order.name,
                order.order_id
            );

            let result = if self.dry_run_mode {
                self.runtime.block_on(api.simulate_invoice_creation(order))
            } else {
                self.runtime.block_on(api.create_invoice(order))
            };

            match result {
                Ok(invoice_result) => {
                    if let Some(ref err) = invoice_result.error {
                        error!(
                            "Failed to {} invoice for {}: {}",
                            if self.dry_run_mode {
                                "simulate"
                            } else {
                                "create"
                            },
                            order.name,
                            err
                        );
                    } else {
                        let action = if self.dry_run_mode {
                            "Simulated"
                        } else {
                            "Successfully created"
                        };
                        info!(
                            "{} invoice for {}: {}",
                            action,
                            order.name,
                            invoice_result
                                .invoice_number
                                .as_ref()
                                .unwrap_or(&"[DRY RUN]".to_string())
                        );
                    }

                    // Execute workflow if invoice was created successfully
                    let mut final_result = invoice_result;
                    if let Some(invoice_id) = final_result.invoice_id {
                        if final_result.error.is_none() {
                            let workflow_options =
                                self.build_workflow_options_with_date(&order.date_of_purchase);
                            if workflow_options.finalize
                                || workflow_options.enshrine
                                || workflow_options.book
                            {
                                let invoice_number =
                                    final_result.invoice_number.as_deref().unwrap_or("Unknown");

                                let workflow_status = if self.dry_run_mode {
                                    self.runtime.block_on(api.simulate_invoice_workflow(
                                        invoice_id,
                                        invoice_number,
                                        &workflow_options,
                                    ))
                                } else {
                                    self.runtime.block_on(api.execute_invoice_workflow(
                                        invoice_id,
                                        invoice_number,
                                        &workflow_options,
                                    ))
                                };

                                // Check for workflow errors
                                if let Some(ref err) = workflow_status.workflow_error {
                                    error!("Workflow error for {}: {}", order.name, err);
                                }

                                final_result.workflow_status = Some(workflow_status);
                            }
                        }
                    }

                    self.results.push(final_result);
                }
                Err(e) => {
                    error!(
                        "Error {} invoice for {}: {}",
                        if self.dry_run_mode {
                            "simulating"
                        } else {
                            "processing"
                        },
                        order.name,
                        e
                    );
                    self.results.push(InvoiceCreationResult {
                        order_id: order.order_id.clone(),
                        customer_name: order.name.clone(),
                        invoice_id: None,
                        invoice_number: None,
                        error: Some(e.to_string()),
                        workflow_status: None,
                    });
                }
            }

            // Update progress
            self.processing_state = ProcessingState::Processing {
                current: index + 1,
                total: self.orders.len(),
            };
        }

        let success_count = self.results.iter().filter(|r| r.error.is_none()).count();
        let error_count = self.results.len() - success_count;
        let action = if self.dry_run_mode {
            "simulation"
        } else {
            "processing"
        };
        info!("Invoice {action} completed: {success_count} successful, {error_count} errors");

        self.processing_state = ProcessingState::Completed;
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

    /// Builds workflow options with a specific payment date (from order)
    fn build_workflow_options_with_date(&self, payment_date: &str) -> InvoiceWorkflowOptions {
        let mut options = self.build_workflow_options();
        options.payment_date = Some(payment_date.to_string());
        options
    }
}
