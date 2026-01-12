//! Invoice workflow management - finalize, enshrine, and book invoices.

use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use std::path::Path;

use crate::models::{InvoiceWorkflowOptions, InvoiceWorkflowStatus, SendType};

use super::SevDeskApi;

impl SevDeskApi {
    /// Executes the invoice workflow steps based on the provided options.
    ///
    /// This will:
    /// 1. Finalize the invoice (mark as sent) if `options.finalize` is true
    /// 2. Download PDF if send type is VPDF and a download path is specified
    /// 3. Enshrine the invoice (lock from changes) if `options.enshrine` is true
    /// 4. Book the invoice against a check account if `options.book` is true
    pub async fn execute_invoice_workflow(
        &self,
        invoice_id: u32,
        invoice_number: &str,
        options: &InvoiceWorkflowOptions,
    ) -> InvoiceWorkflowStatus {
        let mut status = InvoiceWorkflowStatus::default();

        info!(
            "Executing invoice workflow for invoice #{} (ID: {})",
            invoice_number, invoice_id
        );
        debug!("Workflow options: {:?}", options);

        // Step 1: Finalize (mark as sent)
        if options.finalize {
            match self.finalize_invoice(invoice_id, &options.send_type).await {
                Ok(()) => {
                    info!("Invoice #{} finalized successfully", invoice_number);
                    status.finalized = true;
                }
                Err(e) => {
                    error!("Failed to finalize invoice #{}: {}", invoice_number, e);
                    status.workflow_error = Some(format!("Finalize failed: {}", e));
                    return status;
                }
            }

            // Step 1b: Download PDF if VPDF and path is specified
            if options.send_type == SendType::Vpdf {
                if let Some(download_path) = &options.pdf_download_path {
                    match self
                        .download_invoice_pdf(invoice_id, invoice_number, download_path)
                        .await
                    {
                        Ok(pdf_path) => {
                            info!("Invoice #{} PDF saved to {:?}", invoice_number, pdf_path);
                            status.pdf_path = Some(pdf_path);
                        }
                        Err(e) => {
                            // PDF download failure is non-fatal - log warning but continue
                            warn!(
                                "Failed to download PDF for invoice #{}: {}",
                                invoice_number, e
                            );
                        }
                    }
                }
            }
        }

        // Step 2: Enshrine (lock from changes) - requires finalized first
        if options.enshrine {
            if !options.finalize && !status.finalized {
                warn!(
                    "Cannot enshrine invoice #{} without finalizing first",
                    invoice_number
                );
                status.workflow_error =
                    Some("Cannot enshrine: invoice must be finalized first".to_string());
                return status;
            }

            match self.enshrine_invoice(invoice_id).await {
                Ok(()) => {
                    info!("Invoice #{} enshrined successfully", invoice_number);
                    status.enshrined = true;
                }
                Err(e) => {
                    error!("Failed to enshrine invoice #{}: {}", invoice_number, e);
                    status.workflow_error = Some(format!("Enshrine failed: {}", e));
                    return status;
                }
            }
        }

        // Step 3: Book against check account
        if options.book {
            if !options.finalize && !status.finalized {
                warn!(
                    "Cannot book invoice #{} without finalizing first",
                    invoice_number
                );
                status.workflow_error =
                    Some("Cannot book: invoice must be finalized first".to_string());
                return status;
            }

            let check_account_id = match &options.check_account_id {
                Some(id) => id,
                None => {
                    error!(
                        "Cannot book invoice #{}: no check account selected",
                        invoice_number
                    );
                    status.workflow_error =
                        Some("Cannot book: no check account selected".to_string());
                    return status;
                }
            };

            match self
                .book_invoice(
                    invoice_id,
                    check_account_id,
                    options.payment_date.as_deref(),
                )
                .await
            {
                Ok(()) => {
                    info!("Invoice #{} booked successfully", invoice_number);
                    status.booked = true;
                }
                Err(e) => {
                    error!("Failed to book invoice #{}: {}", invoice_number, e);
                    status.workflow_error = Some(format!("Book failed: {}", e));
                    return status;
                }
            }
        }

        status
    }

    /// Finalizes an invoice by marking it as sent.
    ///
    /// This changes the invoice status from DRAFT (100) to OPEN (200).
    pub async fn finalize_invoice(&self, invoice_id: u32, send_type: &SendType) -> Result<()> {
        info!(
            "Finalizing invoice ID {} with send type: {}",
            invoice_id,
            send_type.as_str()
        );

        let url = format!("{}/Invoice/{}/sendBy", self.base_url, invoice_id);
        debug!("Finalize URL: {}", url);

        let body = serde_json::json!({
            "sendType": send_type.as_str(),
            "sendDraft": false
        });

        let response = self
            .client
            .put(&url)
            .header("Authorization", &self.api_token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send finalize request")?;

        let status = response.status();
        debug!("Finalize response status: {}", status);

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("Finalize failed: {} - {}", status, error_text);
            anyhow::bail!("Failed to finalize invoice: {} - {}", status, error_text);
        }

        let response_text = response.text().await.unwrap_or_default();
        debug!("Finalize response: {}", response_text);

        Ok(())
    }

    /// Enshrines an invoice, making it immutable.
    ///
    /// This operation cannot be undone. The invoice must be in OPEN status (200) or higher.
    pub async fn enshrine_invoice(&self, invoice_id: u32) -> Result<()> {
        info!("Enshrining invoice ID {}", invoice_id);

        let url = format!("{}/Invoice/{}/enshrine", self.base_url, invoice_id);
        debug!("Enshrine URL: {}", url);

        let response = self
            .client
            .put(&url)
            .header("Authorization", &self.api_token)
            .send()
            .await
            .context("Failed to send enshrine request")?;

        let status = response.status();
        debug!("Enshrine response status: {}", status);

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("Enshrine failed: {} - {}", status, error_text);
            anyhow::bail!("Failed to enshrine invoice: {} - {}", status, error_text);
        }

        let response_text = response.text().await.unwrap_or_default();
        debug!("Enshrine response: {}", response_text);

        Ok(())
    }

    /// Books an invoice against a check account, marking it as paid.
    ///
    /// This changes the invoice status to PAID (1000).
    /// If `payment_date` is provided (format: "DD.MM.YYYY"), it will be used as the booking date.
    /// Otherwise, the current date is used.
    pub async fn book_invoice(
        &self,
        invoice_id: u32,
        check_account_id: &str,
        payment_date: Option<&str>,
    ) -> Result<()> {
        info!(
            "Booking invoice ID {} against check account {}",
            invoice_id, check_account_id
        );

        // First, get the invoice to know the amount
        let invoice_amount = self.get_invoice_amount(invoice_id).await?;
        debug!("Invoice amount to book: {}", invoice_amount);

        let url = format!("{}/Invoice/{}/bookAmount", self.base_url, invoice_id);
        debug!("Book URL: {}", url);

        // Parse payment date or use current time
        let booking_timestamp = if let Some(date_str) = payment_date {
            // Try to parse date in format "DD.MM.YYYY"
            Self::parse_date_to_timestamp(date_str).unwrap_or_else(|| {
                warn!(
                    "Failed to parse payment date '{}', using current time",
                    date_str
                );
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            })
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        };

        debug!("Booking timestamp: {}", booking_timestamp);

        // Parse check account ID as integer (API requires integer, not string)
        let check_account_id_int: i64 = check_account_id
            .parse()
            .context("Invalid check account ID - must be a number")?;

        let body = serde_json::json!({
            "amount": invoice_amount,
            "date": booking_timestamp,
            "type": "N",
            "checkAccount": {
                "id": check_account_id_int,
                "objectName": "CheckAccount"
            }
        });

        debug!(
            "Book request body: {}",
            serde_json::to_string_pretty(&body).unwrap_or_default()
        );

        let response = self
            .client
            .put(&url)
            .header("Authorization", &self.api_token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send book request")?;

        let status = response.status();
        debug!("Book response status: {}", status);

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("Book failed: {} - {}", status, error_text);
            anyhow::bail!("Failed to book invoice: {} - {}", status, error_text);
        }

        let response_text = response.text().await.unwrap_or_default();
        debug!("Book response: {}", response_text);

        Ok(())
    }

    /// Parses a date string to a Unix timestamp.
    /// Supports formats: "DD.MM.YYYY", "YYYY-MM-DD", "YYYY-MM-DD HH:MM:SS"
    fn parse_date_to_timestamp(date_str: &str) -> Option<u64> {
        use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};

        // Try "YYYY-MM-DD HH:MM:SS" format first (ISO with time)
        if let Ok(dt) = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") {
            return Some(Utc.from_utc_datetime(&dt).timestamp() as u64);
        }

        // Try "YYYY-MM-DD" format (ISO date only)
        if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            let datetime = Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0)?);
            return Some(datetime.timestamp() as u64);
        }

        // Try "DD.MM.YYYY" format (German format)
        let parts: Vec<&str> = date_str.split('.').collect();
        if parts.len() == 3 {
            let day: u32 = parts[0].parse().ok()?;
            let month: u32 = parts[1].parse().ok()?;
            let year: i32 = parts[2].parse().ok()?;
            let date = NaiveDate::from_ymd_opt(year, month, day)?;
            let datetime = Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0)?);
            return Some(datetime.timestamp() as u64);
        }

        None
    }
    /// Gets the total amount of an invoice for booking.
    async fn get_invoice_amount(&self, invoice_id: u32) -> Result<f64> {
        debug!("Getting invoice amount for ID {}", invoice_id);

        let url = format!("{}/Invoice/{}", self.base_url, invoice_id);

        let response = self
            .client
            .get(&url)
            .header("Authorization", &self.api_token)
            .send()
            .await
            .context("Failed to get invoice")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to get invoice: {} - {}", status, error_text);
        }

        let response_text = response.text().await.unwrap_or_default();
        debug!("Invoice response: {}", response_text);

        // Parse the response to get the sumGross field
        // The API can return either {"objects": {...}} or {"objects": [{...}]}
        let json: serde_json::Value =
            serde_json::from_str(&response_text).context("Failed to parse invoice response")?;

        // Try different paths to find sumGross
        let invoice_obj = if json["objects"].is_array() {
            json["objects"].get(0)
        } else {
            Some(&json["objects"])
        };

        let amount = if let Some(obj) = invoice_obj {
            // Try to get sumGross as string first, then as number
            if let Some(s) = obj["sumGross"].as_str() {
                s.parse::<f64>().unwrap_or(0.0)
            } else if let Some(n) = obj["sumGross"].as_f64() {
                n
            } else {
                warn!("Could not find sumGross in invoice response, using 0.0");
                0.0
            }
        } else {
            warn!("Could not find invoice object in response");
            0.0
        };

        debug!("Invoice sum gross: {}", amount);
        Ok(amount)
    }

    /// Downloads the PDF of an invoice and saves it to the specified directory.
    ///
    /// Returns the full path to the saved PDF file.
    pub async fn download_invoice_pdf(
        &self,
        invoice_id: u32,
        invoice_number: &str,
        download_dir: &Path,
    ) -> Result<std::path::PathBuf> {
        info!(
            "Downloading PDF for invoice #{} (ID: {}) to {:?}",
            invoice_number, invoice_id, download_dir
        );

        let url = format!(
            "{}/Invoice/{}/getPdf?download=true&preventSendBy=true",
            self.base_url, invoice_id
        );
        debug!("PDF download URL: {}", url);

        let response = self
            .client
            .get(&url)
            .header("Authorization", &self.api_token)
            .send()
            .await
            .context("Failed to send PDF download request")?;

        let status = response.status();
        debug!("PDF download response status: {}", status);

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("PDF download failed: {} - {}", status, error_text);
            anyhow::bail!("Failed to download PDF: {} - {}", status, error_text);
        }

        // Get the response as bytes (API may return raw PDF or JSON with base64)
        let response_bytes = response
            .bytes()
            .await
            .context("Failed to read PDF response bytes")?;

        debug!("PDF response length: {} bytes", response_bytes.len());

        // Check if the response is raw PDF (starts with %PDF) or JSON
        let pdf_bytes = if response_bytes.starts_with(b"%PDF") {
            // Raw PDF binary data
            debug!("Response is raw PDF data");
            response_bytes.to_vec()
        } else {
            // Try to parse as JSON with base64-encoded content
            debug!("Response appears to be JSON, attempting to parse");
            let response_text = String::from_utf8_lossy(&response_bytes);

            let json: serde_json::Value = serde_json::from_str(&response_text)
                .context("Failed to parse PDF response as JSON (and it's not raw PDF data)")?;

            // Get the base64 content
            let pdf_obj = if json["objects"].is_object() {
                &json["objects"]
            } else {
                &json
            };

            let content = pdf_obj["content"]
                .as_str()
                .context("PDF content not found in JSON response")?;

            // Decode base64 content
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(content)
                .context("Failed to decode PDF base64 content")?
        };

        // Ensure download directory exists
        std::fs::create_dir_all(download_dir).context("Failed to create PDF download directory")?;

        // Save the PDF
        let filename = format!("{}.pdf", invoice_number);
        let pdf_path = download_dir.join(&filename);
        std::fs::write(&pdf_path, &pdf_bytes)
            .context(format!("Failed to write PDF to {:?}", pdf_path))?;

        info!(
            "Successfully saved PDF for invoice #{} to {:?} ({} bytes)",
            invoice_number,
            pdf_path,
            pdf_bytes.len()
        );

        Ok(pdf_path)
    }

    /// Simulates the invoice workflow without making actual API calls.
    pub async fn simulate_invoice_workflow(
        &self,
        invoice_id: u32,
        invoice_number: &str,
        options: &InvoiceWorkflowOptions,
    ) -> InvoiceWorkflowStatus {
        let mut status = InvoiceWorkflowStatus::default();

        info!(
            "[DRY RUN] Simulating invoice workflow for invoice #{} (ID: {})",
            invoice_number, invoice_id
        );
        debug!("[DRY RUN] Workflow options: {:?}", options);

        // Simulate Step 1: Finalize
        if options.finalize {
            info!(
                "[DRY RUN] Would finalize invoice #{} with send type: {}",
                invoice_number,
                options.send_type.as_str()
            );
            status.finalized = true;

            // Simulate Step 1b: PDF download
            if options.send_type == SendType::Vpdf {
                if let Some(download_path) = &options.pdf_download_path {
                    info!(
                        "[DRY RUN] Would download PDF for invoice #{} to {:?}",
                        invoice_number, download_path
                    );
                    status.pdf_path = Some(download_path.join(format!("{}.pdf", invoice_number)));
                }
            }
        }

        // Simulate Step 2: Enshrine
        if options.enshrine {
            if !options.finalize && !status.finalized {
                warn!(
                    "[DRY RUN] Cannot enshrine invoice #{} without finalizing first",
                    invoice_number
                );
                status.workflow_error =
                    Some("Cannot enshrine: invoice must be finalized first".to_string());
                return status;
            }
            info!("[DRY RUN] Would enshrine invoice #{}", invoice_number);
            status.enshrined = true;
        }

        // Simulate Step 3: Book
        if options.book {
            if !options.finalize && !status.finalized {
                warn!(
                    "[DRY RUN] Cannot book invoice #{} without finalizing first",
                    invoice_number
                );
                status.workflow_error =
                    Some("Cannot book: invoice must be finalized first".to_string());
                return status;
            }

            match &options.check_account_id {
                Some(id) => {
                    info!(
                        "[DRY RUN] Would book invoice #{} against check account {}",
                        invoice_number, id
                    );
                    status.booked = true;
                }
                None => {
                    error!(
                        "[DRY RUN] Cannot book invoice #{}: no check account selected",
                        invoice_number
                    );
                    status.workflow_error =
                        Some("Cannot book: no check account selected".to_string());
                    return status;
                }
            };
        }

        status
    }
}
