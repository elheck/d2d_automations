mod logic;
mod ui;

use crate::models::{CheckAccountResponse, InvoiceCreationResult, OrderRecord, SendType};

use std::path::PathBuf;
use tokio::runtime::Runtime;

#[derive(Debug, Clone)]
pub enum ProcessingState {
    Idle,
    LoadingCsv,
    Processing { current: usize, total: usize },
    Completed,
}

pub struct InvoiceApp {
    api_token: String,
    csv_file_path: Option<PathBuf>,
    orders: Vec<OrderRecord>,
    processing_state: ProcessingState,
    results: Vec<InvoiceCreationResult>,
    api_connection_status: Option<bool>,
    runtime: Runtime,
    validation_errors: Vec<String>,
    dry_run_mode: bool,
    // Check account (Verrechnungskonto) fields
    check_accounts: Vec<CheckAccountResponse>,
    selected_check_account_index: Option<usize>,
    check_accounts_loading: bool,
    check_accounts_error: Option<String>,
    // Workflow options
    workflow_finalize: bool,
    workflow_send_type: SendType,
    workflow_enshrine: bool,
    workflow_book: bool,
    // PDF download folder
    pdf_download_path: Option<PathBuf>,
    // Order preview window
    show_order_preview: bool,
}

impl Default for InvoiceApp {
    fn default() -> Self {
        log::info!("Initializing InvoiceApp");
        let api_token = std::env::var("SEVDESK_API").unwrap_or_default();

        if api_token.is_empty() {
            log::warn!("SEVDESK_API environment variable not set");
        } else {
            log::info!("SEVDESK_API environment variable found");
        }

        log::debug!("Creating Tokio runtime");
        let runtime = Runtime::new().expect("Failed to create Tokio runtime");

        Self {
            api_token,
            csv_file_path: None,
            orders: Vec::new(),
            processing_state: ProcessingState::Idle,
            results: Vec::new(),
            api_connection_status: None,
            runtime,
            validation_errors: Vec::new(),
            dry_run_mode: false,
            // Check account fields
            check_accounts: Vec::new(),
            selected_check_account_index: None,
            check_accounts_loading: false,
            check_accounts_error: None,
            // Workflow options - default to false
            workflow_finalize: false,
            workflow_send_type: SendType::default(),
            workflow_enshrine: false,
            workflow_book: false,
            // PDF download path - default to None
            pdf_download_path: None,
            // Order preview window - default to closed
            show_order_preview: false,
        }
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
