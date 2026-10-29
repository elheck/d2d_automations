//! Invoice drafts for Cardmarket orders.

use anyhow::{Context, Result};
use log::{debug, info};

use crate::csv_processor::field_parsers::parse_price;
use crate::models::{
    OrderRecord, SevDeskContactRef, SevDeskCountry, SevDeskInvoice, SevDeskTaxRule, SevDeskUser,
};

use super::draft::{planned_total_cents, to_cents, InvoiceDraft, PlannedPosition};
use super::SevDeskApi;

/// Plans the line items of an order's invoice: one per article plus shipping.
///
/// Orders without parsed items fall back to a single position for the whole
/// merchandise value.
pub(crate) fn plan_order_positions(order: &OrderRecord) -> Result<Vec<PlannedPosition>> {
    let merchandise_value = parse_price(&order.merchandise_value)?;
    let shipment_costs = parse_price(&order.shipment_costs)?;

    let mut positions: Vec<PlannedPosition> = if order.items.is_empty() {
        // The merchandise value already covers every article, so bill it once.
        vec![PlannedPosition {
            number: 1,
            name: order.localized_product_name.clone(),
            text: order.description.clone(),
            quantity: 1.0,
            unit_price: merchandise_value,
        }]
    } else {
        order
            .items
            .iter()
            .zip(1..)
            .map(|(item, number)| PlannedPosition {
                number,
                name: item.localized_product_name.clone(),
                text: item.description.clone(),
                quantity: f64::from(item.quantity),
                unit_price: item.price,
            })
            .collect()
    };

    if shipment_costs > 0.0 {
        positions.push(PlannedPosition {
            number: positions.len() as u32 + 1,
            name: "Shipping".to_string(),
            text: "Shipping costs".to_string(),
            quantity: 1.0,
            unit_price: shipment_costs,
        });
    }

    Ok(positions)
}

/// The amount an order's invoice must add up to: the CSV total, in cents.
pub(crate) fn order_expected_cents(order: &OrderRecord) -> Result<i64> {
    let total = parse_price(&order.total_value).context("Invalid order total")?;
    Ok(to_cents(total))
}

impl SevDeskApi {
    /// Creates a draft invoice for an order and adds its line items.
    ///
    /// Rejected line items are recorded on the returned draft instead of
    /// failing the call, so they can be retried. An error means no usable
    /// draft exists.
    pub(crate) async fn create_order_draft(&self, order: &OrderRecord) -> Result<InvoiceDraft> {
        info!(
            "Creating invoice for order: {} ({})",
            order.order_id, order.name
        );

        // Plan first: bad order data fails before anything is written.
        let positions = plan_order_positions(order)?;
        let expected_cents = order_expected_cents(order)?;

        let contact_id = self.get_or_create_contact(order).await?;
        let user_id = self.get_current_user().await?;
        let country_id = self.get_country_id(&order.country).await?;

        let formatted_address = format!(
            "{}\n{}\n{} {}",
            order.name, order.street, order.zip, order.city
        );
        let date = order
            .date_of_purchase
            .split(' ')
            .next()
            .unwrap_or("")
            .to_string();

        let invoice = SevDeskInvoice {
            invoice_number: None, // Let SevDesk auto-generate
            contact: SevDeskContactRef {
                id: contact_id,
                object_name: "Contact".to_string(),
            },
            invoice_date: date.clone(),
            header: format!("Rechnung für Bestellnummer {}", order.order_id),
            head_text: Some("Vielen Dank für Ihre Bestellung.".to_string()),
            foot_text: Some("Betrag beglichen.".to_string()),
            address: Some(formatted_address),
            address_country: SevDeskCountry {
                id: country_id,
                object_name: "StaticCountry".to_string(),
            },
            delivery_date: date,
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

        let (invoice_id, invoice_number) = self.post_invoice(&invoice).await?;

        debug!(
            "Adding {} positions to invoice #{invoice_number}",
            positions.len()
        );
        let failed = self.add_positions(invoice_id, &positions).await;

        Ok(InvoiceDraft {
            invoice_id,
            invoice_number,
            expected_cents,
            planned_cents: planned_total_cents(&positions),
            failed,
        })
    }
}

#[cfg(test)]
#[path = "invoices_tests.rs"]
mod tests;
