//! Dry-run simulation of invoice creation.

use anyhow::{Context, Result};
use log::{debug, error, info};

use crate::models::{InvoiceCreationResult, OrderRecord};

use super::draft::{format_cents, planned_total_cents};
use super::invoices::{order_expected_cents, plan_order_positions};
use super::SevDeskApi;

/// Dry-run error for a country SevDesk does not know.
pub(crate) fn unknown_country_hint(country: &str) -> String {
    format!(
        "Unknown country '{country}': SevDesk has no country with this name. \
         A real run will ask you to pick the right one."
    )
}

impl SevDeskApi {
    /// Simulates invoice creation without making actual API calls.
    ///
    /// This is useful for validating data before creating real invoices.
    pub async fn simulate_invoice_creation(
        &self,
        order: &OrderRecord,
    ) -> Result<InvoiceCreationResult> {
        info!(
            "Simulating invoice creation for order: {} ({})",
            order.order_id, order.name
        );
        let order_id = order.order_id.clone();
        let customer_name = order.name.clone();

        // Simulate the validation steps without actually making API calls
        match self.simulate_invoice_validation(order).await {
            Ok(simulated_invoice_number) => {
                info!("Successfully simulated invoice: {simulated_invoice_number} for order {order_id}");
                Ok(InvoiceCreationResult {
                    order_id,
                    customer_name,
                    invoice_id: Some(99999), // Fake ID for dry run
                    invoice_number: Some(simulated_invoice_number),
                    error: None,
                    workflow_status: None,
                })
            }
            Err(e) => {
                error!("Failed to simulate invoice for order {order_id}: {e}");
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

    /// Validates order data for invoice creation without making API calls.
    pub(crate) async fn simulate_invoice_validation(&self, order: &OrderRecord) -> Result<String> {
        debug!(
            "Simulating invoice validation for order: {}",
            order.order_id
        );

        // Validate country mapping
        let country_id = self
            .lookup_country_id(&order.country)
            .await?
            .with_context(|| unknown_country_hint(&order.country))?;
        debug!(
            "Country '{}' would map to ID: {}",
            order.country, country_id
        );

        // Validate price parsing and the line items a real run would add
        let positions = plan_order_positions(order)?;
        let expected_cents = order_expected_cents(order)?;
        debug!("Would create {} invoice positions:", positions.len());
        for position in &positions {
            debug!(
                "  Position {}: {} x {} @ {:.2} EUR",
                position.number, position.quantity, position.name, position.unit_price
            );
        }

        // A real run would stop at this invoice and ask, so report it now.
        let planned_cents = planned_total_cents(&positions);
        if planned_cents != expected_cents {
            anyhow::bail!(
                "Line items add up to {} {} but the order total is {} {}",
                format_cents(planned_cents),
                order.currency,
                format_cents(expected_cents),
                order.currency
            );
        }

        // Generate a simulated invoice number
        let simulated_invoice_number = format!("DRY-{}", order.order_id);
        debug!("Simulated invoice number: {simulated_invoice_number}");

        Ok(simulated_invoice_number)
    }
}
