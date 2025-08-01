use eframe::egui;
use log::{debug, error, info, warn};
use std::path::PathBuf;
use tokio::runtime::Runtime;

use crate::{
    csv_processor::CsvProcessor,
    models::{InvoiceCreationResult, OrderRecord},
    sevdesk_api::SevDeskApi,
};

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
    dry_run_mode: bool, // New field for dry-run mode
}

impl Default for InvoiceApp {
    fn default() -> Self {
        info!("Initializing InvoiceApp");
        let api_token = std::env::var("SEVDESK_API").unwrap_or_default();

        if api_token.is_empty() {
            warn!("SEVDESK_API environment variable not set");
        } else {
            info!("SEVDESK_API environment variable found");
        }

        debug!("Creating Tokio runtime");
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
            dry_run_mode: false, // Default to false (actual mode)
        }
    }
}

impl eframe::App for InvoiceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("SevDesk Invoice Creator");
                    ui.add_space(20.0);
                });

                // API Token Section
                ui.group(|ui| {
                    ui.label("SevDesk API Token:");
                    ui.horizontal(|ui| {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.api_token)
                                .password(true)
                                .desired_width(400.0)
                                .hint_text("Enter your SevDesk API token"),
                        );

                        if response.changed() {
                            debug!("API token changed, resetting connection status");
                            self.api_connection_status = None;
                        }

                        if ui
                            .button("Test Connection")
                            .on_disabled_hover_text("Enter API token first")
                            .clicked()
                            && !self.api_token.is_empty()
                        {
                            info!("Testing API connection");
                            self.test_api_connection();
                        }

                        match self.api_connection_status {
                            Some(true) => {
                                ui.colored_label(egui::Color32::GREEN, "✓ Connected");
                            }
                            Some(false) => {
                                ui.colored_label(egui::Color32::RED, "✗ Connection failed");
                            }
                            None => {}
                        }
                    });
                });

                ui.add_space(20.0);

                // CSV File Section
                ui.group(|ui| {
                    ui.label("CSV File:");
                    ui.horizontal(|ui| {
                        if ui.button("Select CSV File").clicked() {
                            info!("Opening file dialog for CSV selection");
                            self.load_csv_file();
                        }

                        if let Some(path) = &self.csv_file_path {
                            ui.label(format!(
                                "Selected: {}",
                                path.file_name().unwrap_or_default().to_string_lossy()
                            ));
                        } else {
                            ui.label("No file selected");
                        }
                    });

                    // Validation errors
                    if !self.validation_errors.is_empty() {
                        ui.separator();
                        ui.colored_label(egui::Color32::RED, "Validation Errors:");
                        egui::ScrollArea::vertical()
                            .max_height(100.0)
                            .show(ui, |ui| {
                                for error in &self.validation_errors {
                                    ui.colored_label(egui::Color32::RED, error);
                                }
                            });
                    }

                    // Orders loaded info
                    if !self.orders.is_empty() {
                        ui.colored_label(
                            egui::Color32::GREEN,
                            format!("Loaded {} orders", self.orders.len()),
                        );
                    }
                });

                ui.add_space(20.0);

                // Processing Section
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.dry_run_mode, "Dry Run Mode")
                            .on_hover_text("Enable to simulate invoice creation without actually creating invoices in SevDesk");

                        if self.dry_run_mode {
                            ui.colored_label(egui::Color32::YELLOW, "⚠ Dry Run: No invoices will be created");
                        }
                    });

                    ui.separator();

                    match &self.processing_state {
                    ProcessingState::Idle => {
                        let can_process = !self.orders.is_empty()
                            && !self.api_token.is_empty()
                            && (self.dry_run_mode || self.api_connection_status == Some(true));

                        let button_text = if self.dry_run_mode {
                            "Simulate Invoice Creation (Dry Run)"
                        } else {
                            "Create Invoices"
                        };

                        if ui
                            .add_enabled(can_process, egui::Button::new(button_text))
                            .on_disabled_hover_text("Load CSV file and test API connection first (or enable dry run mode)")
                            .clicked()
                        {
                            info!(
                                "Starting invoice {} for {} orders",
                                if self.dry_run_mode { "simulation" } else { "creation process" },
                                self.orders.len()
                            );
                            self.process_invoices();
                        }
                    }
                    ProcessingState::LoadingCsv => {
                        ui.label("Loading CSV file...");
                        ui.add(egui::ProgressBar::new(0.0).animate(true));
                    }
                    ProcessingState::Processing { current, total } => {
                        let action = if self.dry_run_mode { "Simulating" } else { "Processing" };
                        ui.label(format!("{action} invoices... ({current}/{total})"));
                        let progress = *current as f32 / *total as f32;
                        ui.add(egui::ProgressBar::new(progress));
                    }
                    ProcessingState::Completed => {
                        let message = if self.dry_run_mode {
                            "Simulation completed!"
                        } else {
                            "Processing completed!"
                        };
                        ui.colored_label(egui::Color32::GREEN, message);
                        if ui.button("Clear Results").clicked() {
                            info!("Clearing processing results");
                            self.results.clear();
                            self.processing_state = ProcessingState::Idle;
                        }
                    }
                    } // Close the match statement
                });

                ui.add_space(20.0);

                // Results Section
                if !self.results.is_empty() {
                    ui.group(|ui| {
                        let success_count =
                            self.results.iter().filter(|r| r.error.is_none()).count();
                        let error_count = self.results.len() - success_count;

                        ui.label(format!(
                            "Results: {success_count} successful, {error_count} errors"
                        ));

                        egui::ScrollArea::vertical()
                            .max_height(200.0)
                            .show(ui, |ui| {
                                for result in &self.results {
                                    let (text, color) = match &result.error {
                                        None => (
                                            format!(
                                                "✓ {} - Invoice #{}",
                                                result.customer_name,
                                                result
                                                    .invoice_number
                                                    .as_ref()
                                                    .unwrap_or(&"Unknown".to_string())
                                            ),
                                            egui::Color32::GREEN,
                                        ),
                                        Some(error) => (
                                            format!(
                                                "✗ {} - Error: {}",
                                                result.customer_name, error
                                            ),
                                            egui::Color32::RED,
                                        ),
                                    };
                                    ui.colored_label(color, text);
                                }
                            });
                    });
                }
            });
        });
    }
}

impl InvoiceApp {
    fn test_api_connection(&mut self) {
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

    fn load_csv_file(&mut self) {
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

    fn process_invoices(&mut self) {
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
                // Dry run mode - simulate the invoice creation
                self.runtime.block_on(api.simulate_invoice_creation(order))
            } else {
                // Normal mode - actually create the invoice
                self.runtime.block_on(api.create_invoice(order))
            };

            match result {
                Ok(invoice_result) => {
                    if invoice_result.error.is_none() {
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
                    } else {
                        error!(
                            "Failed to {} invoice for {}: {}",
                            if self.dry_run_mode {
                                "simulate"
                            } else {
                                "create"
                            },
                            order.name,
                            invoice_result.error.as_ref().unwrap()
                        );
                    }
                    self.results.push(invoice_result);
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
        info!(
            "Invoice {} completed: {success_count} successful, {error_count} errors",
            action
        );

        self.processing_state = ProcessingState::Completed;
    }
}
