//! Draft invoices whose total is verified before any workflow step.
//!
//! Every invoice is first created as a SevDesk draft. Its line items are added
//! one by one, and the ones SevDesk rejects are collected instead of ignored.
//! SevDesk's own total is then compared with the expected total (the order
//! total from the CSV, or the CardTrader report total). Only a draft whose
//! total matches may be finalized, enshrined or booked. Anything else becomes a
//! [`DraftProblem`] that the user resolves.

use anyhow::{Context, Result};
use log::{debug, error, info, warn};

use crate::models::{
    InvoiceResponse, SevDeskInvoice, SevDeskInvoicePos, SevDeskInvoiceRef,
    SevDeskSingleObjectResponse, SevDeskUnity,
};

use super::SevDeskApi;

/// Longest SevDesk error text kept for display (error pages can be huge).
const MAX_ERROR_LEN: usize = 300;

/// Converts a euro amount to whole cents.
pub fn to_cents(amount: f64) -> i64 {
    (amount * 100.0).round() as i64
}

/// Formats cents as a euro amount with two decimals, e.g. `-5.00`.
pub fn format_cents(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

/// One line item to add to an invoice.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedPosition {
    /// Position number on the invoice (1-based).
    pub number: u32,
    pub name: String,
    pub text: String,
    pub quantity: f64,
    /// Unit price in euros (gross = net, no VAT under §19 UStG).
    pub unit_price: f64,
}

impl PlannedPosition {
    /// Line total in cents.
    pub fn total_cents(&self) -> i64 {
        to_cents(self.quantity * self.unit_price)
    }
}

/// Sum of all line totals, in cents.
pub fn planned_total_cents(positions: &[PlannedPosition]) -> i64 {
    positions.iter().map(PlannedPosition::total_cents).sum()
}

/// A line item SevDesk did not accept.
#[derive(Debug, Clone, PartialEq)]
pub struct FailedPosition {
    pub position: PlannedPosition,
    pub error: String,
}

/// A draft invoice in SevDesk and what it should add up to.
#[derive(Debug, Clone, PartialEq)]
pub struct InvoiceDraft {
    pub invoice_id: u32,
    pub invoice_number: String,
    /// What the invoice must add up to (order or report total), in cents.
    pub expected_cents: i64,
    /// What the planned line items add up to, in cents.
    pub planned_cents: i64,
    /// Line items SevDesk rejected so far.
    pub failed: Vec<FailedPosition>,
}

/// Why a draft may not continue to finalize / enshrine / book.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftProblem {
    pub expected_cents: i64,
    pub planned_cents: i64,
    /// SevDesk's total in cents, or why it could not be read.
    pub actual_cents: Result<i64, String>,
    pub failed: Vec<FailedPosition>,
}

impl DraftProblem {
    /// True when SevDesk accepted every line item but they do not add up to
    /// the expected total: the order data is inconsistent, retrying won't help.
    pub fn line_items_disagree_with_total(&self) -> bool {
        self.failed.is_empty() && self.planned_cents != self.expected_cents
    }

    /// One-line description for the results list and logs.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        match &self.actual_cents {
            Ok(actual) => parts.push(format!(
                "SevDesk total {} instead of {}",
                format_cents(*actual),
                format_cents(self.expected_cents)
            )),
            Err(e) => parts.push(format!("SevDesk total could not be read ({e})")),
        }
        if !self.failed.is_empty() {
            parts.push(format!("{} line item(s) rejected", self.failed.len()));
        } else if self.line_items_disagree_with_total() {
            parts.push(format!(
                "the order's line items add up to {}",
                format_cents(self.planned_cents)
            ));
        }
        parts.join(", ")
    }
}

/// Decides whether a draft may continue to the workflow.
///
/// SevDesk's total is what gets invoiced, so it is the only thing that counts:
/// the draft passes exactly when that total equals the expected total.
pub fn check_draft(
    draft: &InvoiceDraft,
    actual_cents: Result<i64, String>,
) -> Option<DraftProblem> {
    if actual_cents.as_ref() == Ok(&draft.expected_cents) {
        if !draft.failed.is_empty() {
            // E.g. a request that timed out although SevDesk saved the position.
            warn!(
                "Invoice #{}: total matches although {} position request(s) failed",
                draft.invoice_number,
                draft.failed.len()
            );
        }
        return None;
    }
    Some(DraftProblem {
        expected_cents: draft.expected_cents,
        planned_cents: draft.planned_cents,
        actual_cents,
        failed: draft.failed.clone(),
    })
}

/// Reads `sumGross` (euros) from a SevDesk invoice response.
///
/// `objects` may be an object or a one-element array, and the value may be a
/// string or a number.
pub(crate) fn parse_sum_gross(json: &serde_json::Value) -> Option<f64> {
    let invoice = if json["objects"].is_array() {
        json["objects"].get(0)?
    } else {
        &json["objects"]
    };
    let value = &invoice["sumGross"];
    value
        .as_str()
        .and_then(|s| s.trim().parse().ok())
        .or_else(|| value.as_f64())
}

/// Shortens an error text for display, keeping whole characters.
fn shorten(text: &str) -> String {
    let text = text.trim();
    match text.char_indices().nth(MAX_ERROR_LEN) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

impl SevDeskApi {
    /// Creates an invoice and returns its ID and number.
    ///
    /// The invoice is expected to be a draft (status 100).
    pub(crate) async fn post_invoice(&self, invoice: &SevDeskInvoice) -> Result<(u32, String)> {
        let url = format!("{}/Invoice", self.base_url);
        debug!("Creating invoice at: {url}");

        let response = self
            .client
            .post(&url)
            .header("Authorization", &self.api_token)
            .header("Content-Type", "application/json")
            .json(invoice)
            .send()
            .await
            .context("Failed to create invoice")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("Invoice creation failed with status {status}: {error_text}");
            anyhow::bail!(
                "Failed to create invoice: {status} - {}",
                shorten(&error_text)
            );
        }

        let response_text = response
            .text()
            .await
            .context("Failed to read response text")?;
        let created: SevDeskSingleObjectResponse<InvoiceResponse> =
            serde_json::from_str(&response_text)
                .context("Failed to parse create invoice response")?;

        let invoice_id = created
            .objects
            .id
            .parse::<u32>()
            .with_context(|| format!("Invalid invoice ID '{}'", created.objects.id))?;
        let invoice_number = created.objects.invoice_number;
        info!("Created draft invoice #{invoice_number} (ID {invoice_id})");
        Ok((invoice_id, invoice_number))
    }

    /// Adds one line item to an invoice. Fails if SevDesk rejects it.
    pub(crate) async fn add_invoice_position(
        &self,
        invoice_id: u32,
        position: &PlannedPosition,
    ) -> Result<()> {
        debug!(
            "Adding position {} to invoice {}: {} x {} @ {:.2}",
            position.number, invoice_id, position.quantity, position.name, position.unit_price
        );

        // Kleinunternehmer (tax rule 11): no VAT, so net = gross.
        let body = SevDeskInvoicePos {
            invoice: SevDeskInvoiceRef {
                id: invoice_id.to_string(),
                object_name: "Invoice".to_string(),
            },
            part: None,
            quantity: position.quantity,
            price: position.unit_price,
            name: position.name.clone(),
            unity: SevDeskUnity {
                id: 1,
                object_name: "Unity".to_string(),
            }, // Piece
            position_number: position.number,
            text: position.text.clone(),
            discount: None,
            tax_rate: 0.0,
            price_net: position.unit_price,
            price_tax: 0.0,
            price_gross: position.unit_price,
        };

        let response = self
            .client
            .post(format!("{}/InvoicePos", self.base_url))
            .header("Authorization", &self.api_token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send invoice position")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("Failed to create invoice position: {status} - {error_text}");
            anyhow::bail!("{status} - {}", shorten(&error_text));
        }
        Ok(())
    }

    /// Adds every position, collecting the ones SevDesk rejects instead of
    /// stopping at the first failure.
    pub(crate) async fn add_positions(
        &self,
        invoice_id: u32,
        positions: &[PlannedPosition],
    ) -> Vec<FailedPosition> {
        let mut failed = Vec::new();
        for position in positions {
            if let Err(e) = self.add_invoice_position(invoice_id, position).await {
                warn!(
                    "Position {} ('{}') was not added to invoice {invoice_id}: {e:#}",
                    position.number, position.name
                );
                failed.push(FailedPosition {
                    position: position.clone(),
                    error: format!("{e:#}"),
                });
            }
        }
        failed
    }

    /// Reads an invoice's `sumGross` in euros; `Ok(None)` when it is missing.
    pub(crate) async fn fetch_invoice_sum_gross(&self, invoice_id: u32) -> Result<Option<f64>> {
        let response = self
            .client
            .get(format!("{}/Invoice/{}", self.base_url, invoice_id))
            .header("Authorization", &self.api_token)
            .send()
            .await
            .context("Failed to get invoice")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to get invoice: {status} - {}", shorten(&error_text));
        }

        let response_text = response.text().await.unwrap_or_default();
        let json: serde_json::Value =
            serde_json::from_str(&response_text).context("Failed to parse invoice response")?;
        Ok(parse_sum_gross(&json))
    }

    /// Reads an invoice's total in cents. A missing total is an error.
    pub(crate) async fn fetch_invoice_total_cents(&self, invoice_id: u32) -> Result<i64> {
        let sum = self
            .fetch_invoice_sum_gross(invoice_id)
            .await?
            .context("SevDesk returned the invoice without a total (sumGross)")?;
        Ok(to_cents(sum))
    }

    /// Compares a draft's total in SevDesk with its expected total.
    ///
    /// Returns `None` when the draft may continue to the workflow.
    pub(crate) async fn verify_draft(&self, draft: &InvoiceDraft) -> Option<DraftProblem> {
        let actual = self
            .fetch_invoice_total_cents(draft.invoice_id)
            .await
            .map_err(|e| format!("{e:#}"));
        let problem = check_draft(draft, actual);
        match &problem {
            None => info!(
                "Invoice #{} total verified: {}",
                draft.invoice_number,
                format_cents(draft.expected_cents)
            ),
            Some(p) => warn!(
                "Invoice #{} is incomplete: {}",
                draft.invoice_number,
                p.summary()
            ),
        }
        problem
    }

    /// Deletes an invoice. Only drafts can be deleted.
    pub(crate) async fn delete_invoice(&self, invoice_id: u32) -> Result<()> {
        info!("Deleting draft invoice ID {invoice_id}");
        let response = self
            .client
            .delete(format!("{}/Invoice/{}", self.base_url, invoice_id))
            .header("Authorization", &self.api_token)
            .send()
            .await
            .context("Failed to send delete request")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("Deleting invoice {invoice_id} failed: {status} - {error_text}");
            anyhow::bail!(
                "Failed to delete invoice: {status} - {}",
                shorten(&error_text)
            );
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "draft_tests.rs"]
mod tests;
