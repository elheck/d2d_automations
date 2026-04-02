//! UI rendering for the InvoiceApp (eframe::App implementation).

use eframe::egui;
use log::info;

use crate::models::SendType;

use super::{InvoiceApp, ProcessingState};

impl eframe::App for InvoiceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.render_order_preview_window(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("SevDesk Invoice Creator");
                    ui.add_space(20.0);
                });

                self.render_api_token_section(ui);
                ui.add_space(20.0);
                self.render_csv_file_section(ui);
                ui.add_space(20.0);
                self.render_check_account_section(ui);
                ui.add_space(20.0);
                self.render_workflow_options_section(ui);
                ui.add_space(20.0);
                self.render_processing_section(ui);
                ui.add_space(20.0);
                self.render_results_section(ui);
            });
        });
    }
}

impl InvoiceApp {
    fn render_api_token_section(&mut self, ui: &mut egui::Ui) {
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
                    log::debug!("API token changed, resetting connection status");
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
    }

    fn render_csv_file_section(&mut self, ui: &mut egui::Ui) {
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
                ui.horizontal(|ui| {
                    ui.colored_label(
                        egui::Color32::GREEN,
                        format!("Loaded {} orders", self.orders.len()),
                    );
                    if ui.button("Review Orders").clicked() {
                        self.show_order_preview = true;
                    }
                });
            }
        });
    }

    fn render_check_account_section(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label("Verrechnungskonto (Check Account):");

            if self.check_accounts_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading accounts...");
                });
            } else if let Some(error) = &self.check_accounts_error {
                ui.colored_label(egui::Color32::RED, format!("Error: {}", error));
                if ui.button("Retry").clicked() {
                    self.load_check_accounts();
                }
            } else if self.check_accounts.is_empty() {
                ui.horizontal(|ui| {
                    ui.label("No accounts loaded.");
                    if self.api_connection_status == Some(true) {
                        if ui.button("Load Accounts").clicked() {
                            self.load_check_accounts();
                        }
                    } else {
                        ui.colored_label(egui::Color32::GRAY, "(Connect to API first)");
                    }
                });
            } else {
                // Dropdown for selecting check account
                let selected_text = self
                    .selected_check_account_index
                    .and_then(|idx| self.check_accounts.get(idx))
                    .map(|acc| acc.display_name())
                    .unwrap_or_else(|| "Select an account...".to_string());

                egui::ComboBox::from_label("")
                    .selected_text(selected_text)
                    .width(350.0)
                    .show_ui(ui, |ui| {
                        for (idx, account) in self.check_accounts.iter().enumerate() {
                            let is_selected = self.selected_check_account_index == Some(idx);
                            let label = if account.is_default() {
                                format!("{} ⭐", account.display_name())
                            } else {
                                account.display_name()
                            };
                            if ui.selectable_label(is_selected, label).clicked() {
                                info!(
                                    "Selected check account: {} (ID: {})",
                                    account.name, account.id
                                );
                                self.selected_check_account_index = Some(idx);
                            }
                        }
                    });

                if self.selected_check_account_index.is_some() {
                    ui.colored_label(egui::Color32::GREEN, "✓ Account selected");
                }
            }
        });
    }

    fn render_workflow_options_section(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label("Invoice Workflow Options:");
            ui.add_space(5.0);

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.workflow_finalize, "Finalize Invoice")
                    .on_hover_text("Mark invoice as sent (DRAFT → OPEN, status 100 → 200)");

                if self.workflow_finalize {
                    ui.label("Send Type:");
                    egui::ComboBox::from_id_salt("send_type_combo")
                        .selected_text(self.workflow_send_type.description())
                        .width(180.0)
                        .show_ui(ui, |ui| {
                            for send_type in SendType::all() {
                                let is_selected = self.workflow_send_type == *send_type;
                                if ui
                                    .selectable_label(is_selected, send_type.description())
                                    .clicked()
                                {
                                    self.workflow_send_type = send_type.clone();
                                }
                            }
                        });
                }
            });

            ui.horizontal(|ui| {
                let enshrine_enabled = self.workflow_finalize;
                ui.add_enabled(
                    enshrine_enabled,
                    egui::Checkbox::new(&mut self.workflow_enshrine, "Enshrine Invoice"),
                )
                .on_hover_text("Lock invoice from changes (irreversible). Requires finalization.");

                if !enshrine_enabled && self.workflow_enshrine {
                    self.workflow_enshrine = false;
                }
            });

            ui.horizontal(|ui| {
                let book_enabled =
                    self.workflow_finalize && self.selected_check_account_index.is_some();
                ui.add_enabled(
                    book_enabled,
                    egui::Checkbox::new(&mut self.workflow_book, "Book Invoice"),
                )
                .on_hover_text("Book invoice as paid against the selected check account. Requires finalization and account selection.");

                if !book_enabled && self.workflow_book {
                    self.workflow_book = false;
                }

                if self.workflow_book && self.selected_check_account_index.is_none() {
                    ui.colored_label(egui::Color32::RED, "⚠ Select check account first");
                }
            });

            // PDF Download Folder (only show when VPDF is selected)
            if self.workflow_finalize && self.workflow_send_type == SendType::Vpdf {
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.label("PDF Download Folder:");
                    if ui.button("Select Folder").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            info!("Selected PDF download folder: {:?}", path);
                            self.pdf_download_path = Some(path);
                        }
                    }

                    if let Some(path) = &self.pdf_download_path {
                        ui.label(path.display().to_string());
                        if ui
                            .button("✕")
                            .on_hover_text("Clear folder selection")
                            .clicked()
                        {
                            self.pdf_download_path = None;
                        }
                    } else {
                        ui.colored_label(
                            egui::Color32::GRAY,
                            "(PDFs will not be downloaded)",
                        );
                    }
                });
            }

            // Show workflow summary
            if self.workflow_finalize || self.workflow_enshrine || self.workflow_book {
                ui.add_space(5.0);
                ui.separator();
                let mut steps = vec!["Create invoice".to_string()];
                if self.workflow_finalize {
                    steps.push(format!(
                        "Finalize ({})",
                        self.workflow_send_type.description()
                    ));
                    if self.workflow_send_type == SendType::Vpdf
                        && self.pdf_download_path.is_some()
                    {
                        steps.push("Download PDF".to_string());
                    }
                }
                if self.workflow_enshrine {
                    steps.push("Enshrine".to_string());
                }
                if self.workflow_book {
                    steps.push("Book payment".to_string());
                }
                ui.colored_label(
                    egui::Color32::LIGHT_BLUE,
                    format!("Workflow: {}", steps.join(" → ")),
                );
            }
        });
    }

    fn render_processing_section(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.dry_run_mode, "Dry Run Mode")
                    .on_hover_text(
                        "Enable to simulate invoice creation without actually creating invoices in SevDesk",
                    );

                if self.dry_run_mode {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        "⚠ Dry Run: No invoices will be created",
                    );
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
                        .on_disabled_hover_text(
                            "Load CSV file and test API connection first (or enable dry run mode)",
                        )
                        .clicked()
                    {
                        info!(
                            "Starting invoice {} for {} orders",
                            if self.dry_run_mode {
                                "simulation"
                            } else {
                                "creation process"
                            },
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
                    let action = if self.dry_run_mode {
                        "Simulating"
                    } else {
                        "Processing"
                    };
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
            }
        });
    }

    fn render_results_section(&self, ui: &mut egui::Ui) {
        if !self.results.is_empty() {
            ui.group(|ui| {
                let success_count = self.results.iter().filter(|r| r.error.is_none()).count();
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
                                    format!("✗ {} - Error: {}", result.customer_name, error),
                                    egui::Color32::RED,
                                ),
                            };
                            ui.colored_label(color, text);
                        }
                    });
            });
        }
    }

    fn render_order_preview_window(&mut self, ctx: &egui::Context) {
        if !self.show_order_preview {
            return;
        }

        let mut open = self.show_order_preview;
        egui::Window::new("Order Preview")
            .open(&mut open)
            .resizable(true)
            .default_size([900.0, 500.0])
            .show(ctx, |ui| {
                ui.label(format!("{} orders to be invoiced:", self.orders.len()));
                ui.add_space(5.0);

                let total: f64 = self
                    .orders
                    .iter()
                    .filter_map(|o| o.total_value.replace(',', ".").parse::<f64>().ok())
                    .sum();
                ui.colored_label(
                    egui::Color32::LIGHT_BLUE,
                    format!("Total value: {total:.2} EUR"),
                );
                ui.add_space(5.0);

                egui::ScrollArea::both().show(ui, |ui| {
                    egui_extras::TableBuilder::new(ui)
                        .striped(true)
                        .resizable(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(egui_extras::Column::auto().at_least(70.0)) // Order ID
                        .column(egui_extras::Column::auto().at_least(80.0)) // Date
                        .column(egui_extras::Column::auto().at_least(120.0)) // Customer
                        .column(egui_extras::Column::auto().at_least(100.0)) // Country
                        .column(egui_extras::Column::auto().at_least(40.0)) // Items
                        .column(egui_extras::Column::auto().at_least(80.0)) // Merchandise
                        .column(egui_extras::Column::auto().at_least(60.0)) // Shipping
                        .column(egui_extras::Column::auto().at_least(70.0)) // Total
                        .column(egui_extras::Column::remainder()) // Description
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.strong("Order ID");
                            });
                            header.col(|ui| {
                                ui.strong("Date");
                            });
                            header.col(|ui| {
                                ui.strong("Customer");
                            });
                            header.col(|ui| {
                                ui.strong("Country");
                            });
                            header.col(|ui| {
                                ui.strong("Items");
                            });
                            header.col(|ui| {
                                ui.strong("Merch.");
                            });
                            header.col(|ui| {
                                ui.strong("Shipping");
                            });
                            header.col(|ui| {
                                ui.strong("Total");
                            });
                            header.col(|ui| {
                                ui.strong("Description");
                            });
                        })
                        .body(|mut body| {
                            for order in &self.orders {
                                let line_count = order.items.len().max(1);
                                let row_height = line_count as f32 * 18.0;
                                body.row(row_height, |mut row| {
                                    row.col(|ui| {
                                        ui.label(&order.order_id);
                                    });
                                    row.col(|ui| {
                                        ui.label(&order.date_of_purchase);
                                    });
                                    row.col(|ui| {
                                        ui.label(&order.name);
                                    });
                                    row.col(|ui| {
                                        ui.label(&order.country);
                                    });
                                    row.col(|ui| {
                                        ui.label(order.article_count.to_string());
                                    });
                                    row.col(|ui| {
                                        ui.label(format!(
                                            "{} {}",
                                            &order.merchandise_value, &order.currency
                                        ));
                                    });
                                    row.col(|ui| {
                                        ui.label(format!(
                                            "{} {}",
                                            &order.shipment_costs, &order.currency
                                        ));
                                    });
                                    row.col(|ui| {
                                        ui.label(format!(
                                            "{} {}",
                                            &order.total_value, &order.currency
                                        ));
                                    });
                                    row.col(|ui| {
                                        ui.vertical(|ui| {
                                            for item in &order.items {
                                                ui.label(format!(
                                                    "• {}x {}",
                                                    item.quantity, item.localized_product_name
                                                ));
                                            }
                                        });
                                    });
                                });
                            }
                        });
                });
            });
        self.show_order_preview = open;
    }
}
