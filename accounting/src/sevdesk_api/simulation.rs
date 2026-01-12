//! Dry-run simulation of invoice creation.

use anyhow::Result;
use log::{debug, error, info};

use crate::models::{InvoiceCreationResult, OrderRecord};

use super::SevDeskApi;

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
        let country_id = self.get_country_id(&order.country).await?;
        debug!(
            "Country '{}' would map to ID: {}",
            order.country, country_id
        );

        // Validate price parsing
        let merchandise_value = self.parse_price(&order.merchandise_value)?;
        let shipment_costs = self.parse_price(&order.shipment_costs)?;
        let total_value = self.parse_price(&order.total_value)?;
        debug!("Prices would be - merchandise: {merchandise_value:.2}, shipping: {shipment_costs:.2}, total: {total_value:.2}");

        // Validate items and quantities
        if !order.items.is_empty() {
            debug!("Would create {} invoice positions:", order.items.len());
            for (i, item) in order.items.iter().enumerate() {
                debug!(
                    "  Position {}: {} x {} @ {:.2} EUR",
                    i + 1,
                    item.quantity,
                    item.localized_product_name,
                    item.price
                );
            }
        } else {
            debug!(
                "Would create fallback position: {} x {} @ {:.2} EUR",
                order.article_count, order.localized_product_name, merchandise_value
            );
        }

        if shipment_costs > 0.0 {
            debug!("Would add shipping position: {shipment_costs:.2} EUR");
        }

        // Generate a simulated invoice number
        let simulated_invoice_number = format!("DRY-{}", order.order_id);
        debug!("Simulated invoice number: {simulated_invoice_number}");

        Ok(simulated_invoice_number)
    }
}
