//! Consolidated invoice creation for CardTrader sales reports.
//!
//! A whole CardTrader report becomes one invoice to one recipient, with each
//! grouped card as an invoice position. Only the net amount is billed -
//! CardTrader invoices its fees separately.

use anyhow::{Context, Result};
use log::{debug, error, info};

use crate::models::{
    AddressCategory, ConsolidatedInvoice, ContactCategory, ContactResponse, InvoiceCreationResult,
    InvoiceRecipient, InvoiceResponse, SevDeskAddress, SevDeskContact, SevDeskContactRef,
    SevDeskCountry, SevDeskInvoice, SevDeskResponse, SevDeskSingleObjectResponse, SevDeskTaxRule,
    SevDeskUser,
};

use super::SevDeskApi;

impl SevDeskApi {
    /// Creates a single invoice covering every position of a CardTrader report.
    pub async fn create_consolidated_invoice(
        &self,
        invoice: &ConsolidatedInvoice,
    ) -> Result<InvoiceCreationResult> {
        info!(
            "Creating consolidated invoice for {} ({} positions, {:.2} {})",
            invoice.recipient.name,
            invoice.positions.len(),
            invoice.total(),
            invoice.currency
        );

        let customer_name = invoice.recipient.name.clone();
        let order_id = format!("CardTrader {}", invoice.period_label);

        match self.create_consolidated_invoice_internal(invoice).await {
            Ok((invoice_id, invoice_number)) => {
                info!("Successfully created consolidated invoice: {invoice_number}");
                Ok(InvoiceCreationResult {
                    order_id,
                    customer_name,
                    invoice_id: Some(invoice_id.parse().unwrap_or(0)),
                    invoice_number: Some(invoice_number),
                    error: None,
                    workflow_status: None,
                })
            }
            Err(e) => {
                error!("Failed to create consolidated invoice: {e}");
                Ok(InvoiceCreationResult {
                    order_id,
                    customer_name,
                    invoice_id: None,
                    invoice_number: None,
                    error: Some(e.to_string()),
                    workflow_status: None,
                })
            }
        }
    }

    /// Internal implementation of consolidated invoice creation.
    pub(crate) async fn create_consolidated_invoice_internal(
        &self,
        invoice: &ConsolidatedInvoice,
    ) -> Result<(String, String)> {
        let contact_id = self
            .get_or_create_recipient_contact(&invoice.recipient)
            .await?;
        let user_id = self.get_current_user().await?;
        let country_id = self.get_country_id(&invoice.recipient.country).await?;

        let sev_invoice = SevDeskInvoice {
            invoice_number: None, // Let SevDesk auto-generate
            contact: SevDeskContactRef {
                id: contact_id,
                object_name: "Contact".to_string(),
            },
            invoice_date: invoice.invoice_date.clone(),
            header: format!("Rechnung CardTrader {}", invoice.period_label),
            head_text: Some("Vielen Dank für Ihre Bestellung.".to_string()),
            foot_text: Some("Betrag beglichen.".to_string()),
            address: Some(invoice.recipient.formatted_address()),
            address_country: SevDeskCountry {
                id: country_id,
                object_name: "StaticCountry".to_string(),
            },
            delivery_date: invoice.invoice_date.clone(),
            status: 100, // Draft status
            small_settlement: false,
            contact_person: SevDeskUser {
                id: user_id,
                object_name: "SevUser".to_string(),
            },
            tax_rate: 0.0, // No VAT for Kleingewerbe
            tax_text: "Kleinunternehmerregelung §19 UStG".to_string(),
            tax_rule: SevDeskTaxRule {
                id: 11, // Tax rule 11 for Kleingewerbe
                object_name: "TaxRule".to_string(),
            },
            dunning_level: None,
            invoice_type: "RE".to_string(), // Regular invoice
            currency: invoice.currency.clone(),
        };

        let create_invoice_url = format!("{}/Invoice", self.base_url);
        debug!("Creating consolidated invoice at: {create_invoice_url}");

        let response = self
            .client
            .post(&create_invoice_url)
            .header("Authorization", &self.api_token)
            .header("Content-Type", "application/json")
            .json(&sev_invoice)
            .send()
            .await
            .context("Failed to create consolidated invoice")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .context("Failed to read error response")?;
            error!("Consolidated invoice creation failed with status {status}: {error_text}");
            return Err(anyhow::anyhow!(
                "Failed to create invoice: {} - {}",
                status,
                error_text
            ));
        }

        let response_text = response
            .text()
            .await
            .context("Failed to read response text")?;

        let created_invoice: SevDeskSingleObjectResponse<InvoiceResponse> =
            serde_json::from_str(&response_text)
                .context("Failed to parse create invoice response")?;

        let invoice_id = created_invoice.objects.id.clone();
        let invoice_number = created_invoice.objects.invoice_number;
        info!("Created consolidated invoice ID {invoice_id} (#{invoice_number})");

        // Add each grouped card as its own position. Unlike the per-order path,
        // a failed position here would silently understate a large invoice, so
        // failures abort instead of being logged and skipped.
        for (index, position) in invoice.positions.iter().enumerate() {
            let position_number = index as u32 + 1;
            debug!(
                "Adding position {}/{}: {} x {} @ {:.2}",
                position_number,
                invoice.positions.len(),
                position.quantity,
                position.display_name(),
                position.unit_price()
            );

            self.add_invoice_position_strict(
                &invoice_id,
                position_number,
                &position.display_name(),
                &position.display_text(),
                position.quantity as f64,
                position.unit_price(),
            )
            .await
            .with_context(|| {
                format!(
                    "Failed to add position {} ('{}') to invoice #{}",
                    position_number,
                    position.display_name(),
                    invoice_number
                )
            })?;
        }

        info!(
            "Added {} positions to invoice #{}",
            invoice.positions.len(),
            invoice_number
        );

        Ok((invoice_id, invoice_number))
    }

    /// Finds or creates the contact for a consolidated invoice recipient.
    ///
    /// Matches on the exact contact name so an edited recipient creates its own
    /// contact rather than silently reusing a similarly named one.
    pub(crate) async fn get_or_create_recipient_contact(
        &self,
        recipient: &InvoiceRecipient,
    ) -> Result<u32> {
        debug!(
            "Getting or creating contact for recipient: {}",
            recipient.name
        );

        let search_url = format!("{}/Contact", self.base_url);
        let response = self
            .client
            .get(&search_url)
            .header("Authorization", &self.api_token)
            .query(&[("name", &recipient.name)])
            .send()
            .await
            .context("Failed to search for recipient contact")?;

        let response_text = response
            .text()
            .await
            .context("Failed to read contact search response")?;

        let contacts: SevDeskResponse<ContactResponse> = serde_json::from_str(&response_text)
            .context("Failed to parse contact search response")?;

        if let Some(existing) = contacts.objects {
            // Require an exact (case-insensitive) name match before reusing.
            if let Some(contact) = existing
                .iter()
                .find(|c| c.name.trim().eq_ignore_ascii_case(recipient.name.trim()))
            {
                let contact_id = contact
                    .id
                    .parse::<u32>()
                    .context("Failed to parse contact ID from string")?;
                info!(
                    "Found existing contact: {} (ID: {})",
                    recipient.name, contact_id
                );
                return Ok(contact_id);
            }
        }

        debug!("No existing contact found, creating new contact");
        let country_id = self.get_country_id(&recipient.country).await?;

        let new_contact = SevDeskContact {
            name: recipient.name.clone(),
            category: ContactCategory {
                id: 3,
                object_name: "Category".to_string(),
            }, // Customer category
            addresses: vec![SevDeskAddress {
                street: recipient.street.clone(),
                zip: recipient.zip.clone(),
                city: recipient.city.clone(),
                country: SevDeskCountry {
                    id: country_id,
                    object_name: "StaticCountry".to_string(),
                },
                category: AddressCategory {
                    id: 47,
                    object_name: "Category".to_string(),
                }, // Billing address
            }],
        };

        let create_url = format!("{}/Contact", self.base_url);
        let response = self
            .client
            .post(&create_url)
            .header("Authorization", &self.api_token)
            .header("Content-Type", "application/json")
            .json(&new_contact)
            .send()
            .await
            .context("Failed to create recipient contact")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .context("Failed to read error response")?;
            error!("Contact creation failed with status {status}: {error_text}");
            return Err(anyhow::anyhow!(
                "Failed to create contact: {} - {}",
                status,
                error_text
            ));
        }

        let response_text = response
            .text()
            .await
            .context("Failed to read response text")?;

        let created_contact: SevDeskSingleObjectResponse<ContactResponse> =
            serde_json::from_str(&response_text)
                .context("Failed to parse create contact response")?;

        let contact_id = created_contact
            .objects
            .id
            .parse::<u32>()
            .context("Failed to parse created contact ID from string")?;

        info!(
            "Created new contact: {} (ID: {})",
            recipient.name, contact_id
        );
        Ok(contact_id)
    }

    /// Simulates consolidated invoice creation without making write API calls.
    pub async fn simulate_consolidated_invoice(
        &self,
        invoice: &ConsolidatedInvoice,
    ) -> Result<InvoiceCreationResult> {
        info!(
            "[DRY RUN] Simulating consolidated invoice for {} ({} positions)",
            invoice.recipient.name,
            invoice.positions.len()
        );

        let customer_name = invoice.recipient.name.clone();
        let order_id = format!("CardTrader {}", invoice.period_label);

        match self.simulate_consolidated_validation(invoice).await {
            Ok(number) => Ok(InvoiceCreationResult {
                order_id,
                customer_name,
                invoice_id: Some(99999), // Fake ID for dry run
                invoice_number: Some(number),
                error: None,
                workflow_status: None,
            }),
            Err(e) => {
                error!("[DRY RUN] Consolidated invoice simulation failed: {e}");
                Ok(InvoiceCreationResult {
                    order_id,
                    customer_name,
                    invoice_id: None,
                    invoice_number: None,
                    error: Some(e.to_string()),
                    workflow_status: None,
                })
            }
        }
    }

    /// Validates a consolidated invoice without creating anything.
    pub(crate) async fn simulate_consolidated_validation(
        &self,
        invoice: &ConsolidatedInvoice,
    ) -> Result<String> {
        let country_id = self.get_country_id(&invoice.recipient.country).await?;
        debug!(
            "[DRY RUN] Country '{}' would map to ID: {}",
            invoice.recipient.country, country_id
        );

        if invoice.positions.is_empty() {
            anyhow::bail!("Consolidated invoice has no positions");
        }

        debug!(
            "[DRY RUN] Would create {} positions totalling {:.2} {}",
            invoice.positions.len(),
            invoice.total(),
            invoice.currency
        );

        for (i, position) in invoice.positions.iter().enumerate() {
            debug!(
                "[DRY RUN]   Position {}: {} x {} @ {:.2}",
                i + 1,
                position.quantity,
                position.display_name(),
                position.unit_price()
            );
        }

        Ok(format!("DRY-CARDTRADER-{}", invoice.invoice_date))
    }
}

#[cfg(test)]
#[path = "consolidated_invoice_tests.rs"]
mod tests;
