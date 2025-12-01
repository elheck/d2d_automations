//! Invoice creation and management.

use anyhow::{Context, Result};
use log::{debug, error, info};

use crate::models::{
    InvoiceCreationResult, InvoiceResponse, OrderRecord, SevDeskContactRef, SevDeskCountry,
    SevDeskInvoice, SevDeskInvoicePos, SevDeskInvoiceRef, SevDeskSingleObjectResponse,
    SevDeskTaxRule, SevDeskUnity, SevDeskUser,
};

use super::SevDeskApi;

impl SevDeskApi {
    /// Creates an invoice for the given order.
    pub async fn create_invoice(&self, order: &OrderRecord) -> Result<InvoiceCreationResult> {
        info!(
            "Creating invoice for order: {} ({})",
            order.order_id, order.name
        );
        let order_id = order.order_id.clone();
        let customer_name = order.name.clone();

        match self.create_invoice_internal(order).await {
            Ok((invoice_id, invoice_number)) => {
                info!("Successfully created invoice: {invoice_number} for order {order_id}");
                Ok(InvoiceCreationResult {
                    order_id,
                    customer_name,
                    invoice_id: Some(invoice_id.parse().unwrap_or(0)),
                    invoice_number: Some(invoice_number),
                    error: None,
                })
            }
            Err(e) => {
                error!("Failed to create invoice for order {order_id}: {e}");
                Ok(InvoiceCreationResult {
                    order_id,
                    customer_name,
                    invoice_id: None,
                    invoice_number: None,
                    error: Some(e.to_string()),
                })
            }
        }
    }

    /// Internal implementation of invoice creation.
    pub(crate) async fn create_invoice_internal(
        &self,
        order: &OrderRecord,
    ) -> Result<(String, String)> {
        debug!(
            "Starting internal invoice creation for order: {}",
            order.order_id
        );

        // Get or create contact
        let contact_id = self.get_or_create_contact(order).await?;

        // Get current user ID
        let user_id = self.get_current_user().await?;

        // Parse prices
        let merchandise_value = self.parse_price(&order.merchandise_value)?;
        let shipment_costs = self.parse_price(&order.shipment_costs)?;
        let _total_value = self.parse_price(&order.total_value)?;

        debug!("Parsed prices - merchandise: {merchandise_value}, shipping: {shipment_costs}");

        // Create invoice
        let country_id = self.get_country_id(&order.country).await?;

        // Format the complete address
        let formatted_address = format!(
            "{}\n{}\n{} {}",
            order.name, order.street, order.zip, order.city
        );

        let invoice = SevDeskInvoice {
            invoice_number: None, // Let SevDesk auto-generate
            contact: SevDeskContactRef {
                id: contact_id,
                object_name: "Contact".to_string(),
            },
            invoice_date: order
                .date_of_purchase
                .split(' ')
                .next()
                .unwrap_or("")
                .to_string(),
            header: format!("Rechnung für Bestellnummer {}", order.order_id),
            head_text: Some("Vielen Dank für Ihre Bestellung.".to_string()),
            foot_text: Some("Betrag beglichen.".to_string()),
            address: Some(formatted_address),
            address_country: SevDeskCountry {
                id: country_id,
                object_name: "StaticCountry".to_string(),
            },
            delivery_date: order
                .date_of_purchase
                .split(' ')
                .next()
                .unwrap_or("")
                .to_string(),
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
            currency: order.currency.clone(),
        };

        let create_invoice_url = format!("{}/Invoice", self.base_url);
        debug!("Creating invoice at: {create_invoice_url}");

        let response = self
            .client
            .post(&create_invoice_url)
            .header("Authorization", &self.api_token)
            .header("Content-Type", "application/json")
            .json(&invoice)
            .send()
            .await
            .context("Failed to create invoice")?;

        debug!("Create invoice response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .context("Failed to read error response")?;
            error!("Invoice creation failed with status {status}: {error_text}");
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
        debug!("Create invoice response body: {response_text}");

        let created_invoice: SevDeskSingleObjectResponse<InvoiceResponse> =
            serde_json::from_str(&response_text)
                .context("Failed to parse create invoice response")?;

        let invoice_id = created_invoice.objects.id.clone();
        let invoice_number = created_invoice.objects.invoice_number;
        debug!("Created invoice with ID: {invoice_id} and number: {invoice_number}");

        // Add each item as a separate invoice position
        let mut position_number = 1;

        if order.items.len() > 1 {
            info!("Adding {} individual items to invoice", order.items.len());
            for item in &order.items {
                debug!(
                    "Adding item position {}: {} x {} @ {:.2} EUR",
                    position_number, item.quantity, item.localized_product_name, item.price
                );
                self.add_invoice_position(
                    &invoice_id,
                    position_number,
                    &item.localized_product_name,
                    &item.description,
                    item.quantity as f64, // Use the extracted quantity from description
                    item.price,
                )
                .await?;
                position_number += 1;
            }
        } else if !order.items.is_empty() {
            // Single item order
            let item = &order.items[0];
            debug!(
                "Adding single item position: {} x {} @ {:.2} EUR",
                item.quantity, item.localized_product_name, item.price
            );
            self.add_invoice_position(
                &invoice_id,
                position_number,
                &item.localized_product_name,
                &item.description,
                item.quantity as f64, // Use the extracted quantity from description
                item.price,
            )
            .await?;
            position_number += 1;
        } else {
            // Fallback to original merchandise value
            debug!(
                "No items found, using fallback merchandise position: {} x {} = {}",
                order.article_count,
                merchandise_value,
                order.article_count as f64 * merchandise_value
            );
            self.add_invoice_position(
                &invoice_id,
                position_number,
                &order.localized_product_name,
                &order.description,
                order.article_count as f64,
                merchandise_value,
            )
            .await?;
            position_number += 1;
        }

        // Add shipping costs as separate position if any
        if shipment_costs > 0.0 {
            debug!("Adding shipping position: {shipment_costs}");
            self.add_invoice_position(
                &invoice_id,
                position_number,
                "Shipping",
                "Shipping costs",
                1.0,
                shipment_costs,
            )
            .await?;
        }

        Ok((invoice_id, invoice_number))
    }

    /// Adds a position (line item) to an invoice.
    pub(crate) async fn add_invoice_position(
        &self,
        invoice_id: &str,
        position_number: u32,
        name: &str,
        description: &str,
        quantity: f64,
        price_gross: f64,
    ) -> Result<()> {
        debug!("Adding invoice position {position_number}: {quantity} x {name} @ {price_gross}");

        // For Kleingewerbe (tax rule 11), no VAT is applied
        let tax_rate = 0.0; // No VAT for Kleingewerbe
        let price_net = price_gross; // Price is the same as gross since no VAT
        let price_tax = 0.0; // No tax

        debug!(
            "Kleingewerbe pricing - net: {price_net:.2}, tax: {price_tax:.2}, gross: {price_gross:.2}"
        );

        let position = SevDeskInvoicePos {
            invoice: SevDeskInvoiceRef {
                id: invoice_id.to_string(),
                object_name: "Invoice".to_string(),
            },
            part: None,
            quantity,
            price: price_net,
            name: name.to_string(),
            unity: SevDeskUnity {
                id: 1,
                object_name: "Unity".to_string(),
            }, // Piece
            position_number,
            text: description.to_string(),
            discount: None,
            tax_rate,
            price_net,
            price_tax,
            price_gross,
        };

        let create_position_url = format!("{}/InvoicePos", self.base_url);
        debug!("Creating invoice position at: {create_position_url}");

        let response = self
            .client
            .post(&create_position_url)
            .header("Authorization", &self.api_token)
            .header("Content-Type", "application/json")
            .json(&position)
            .send()
            .await
            .context("Failed to create invoice position")?;

        debug!("Create position response status: {}", response.status());
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("Failed to create invoice position: {error_text}");
        }

        Ok(())
    }
}
