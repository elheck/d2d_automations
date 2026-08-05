//! CardTrader sales report parsing and consolidation.
//!
//! A CardTrader sales report contains one row per sold item across many buyers
//! and orders. Unlike the Cardmarket export (one invoice per order), the whole
//! report is billed as a *single* invoice to one recipient.
//!
//! # Money handling
//!
//! All amounts in the report are integer cents. They are kept as `i64` through
//! parsing and aggregation so that summing hundreds of rows cannot accumulate
//! floating point error; conversion to `f64` happens only when handing values
//! to the SevDesk API.
//!
//! # Net vs. gross
//!
//! Only the **net** amount is invoiced (`Amount` + `Amount for cancelation`).
//! CardTrader bills its fees separately, so the `CardTrader fee` column is
//! parsed for reporting but deliberately excluded from invoice totals.

use anyhow::{Context, Result};
use log::{debug, info, warn};

use crate::models::{
    CardTraderSaleRow, ConsolidatedInvoice, ConsolidatedPosition, InvoiceRecipient,
};

/// Column headers that uniquely identify a CardTrader sales report.
const REQUIRED_HEADERS: [&str; 4] = [
    "Order Code",
    "Amount (cents)",
    "CardTrader fee (cents)",
    "Item name",
];

/// Returns true when the CSV header line looks like a CardTrader sales report.
///
/// Used to route a loaded file to the CardTrader path without affecting the
/// existing Cardmarket detection.
pub fn is_cardtrader_report(header_line: &str) -> bool {
    REQUIRED_HEADERS
        .iter()
        .all(|needle| header_line.contains(needle))
}

/// Parses raw CardTrader report content into sale rows.
///
/// Uses a proper CSV reader (not `split(';')`) because the `Comment` column is
/// quoted and may itself contain separators.
pub fn parse_sales_report(content: &str) -> Result<Vec<CardTraderSaleRow>> {
    debug!("Parsing CardTrader sales report");

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b';')
        .flexible(true)
        .from_reader(content.as_bytes());

    let headers = reader
        .headers()
        .context("Failed to read CardTrader report headers")?
        .clone();

    let index_of = |name: &str| -> Result<usize> {
        headers
            .iter()
            .position(|h| h.trim() == name)
            .with_context(|| format!("CardTrader report is missing the '{name}' column"))
    };

    let idx_id = index_of("ID")?;
    let idx_order_code = index_of("Order Code")?;
    // Note: the report's column labels are misleading - "Sold at (datetime)" holds
    // a coarse date ("1 Jul 2026") while the plain "Sold at" column holds the full
    // ISO timestamp ("2026-07-01 09:30:30 +0200"). Prefer the ISO one.
    let idx_sold_at = index_of("Sold at").or_else(|_| index_of("Sold at (datetime)"))?;
    let idx_currency = index_of("Currency")?;
    let idx_amount = index_of("Amount (cents)")?;
    let idx_cancelation = index_of("Amount for cancelation (cents)")?;
    let idx_fee = index_of("CardTrader fee (cents)")?;
    let idx_item_name = index_of("Item name")?;
    let idx_expansion = index_of("Item expansion")?;
    let idx_properties = index_of("Item properties")?;

    let mut rows = Vec::new();

    for (record_num, record) in reader.records().enumerate() {
        let line_num = record_num + 2; // +2: 1-indexed plus header row
        let record =
            record.with_context(|| format!("Failed to read CSV record on line {line_num}"))?;

        // A trailing blank line yields an empty record - skip it rather than failing.
        if record.iter().all(|f| f.trim().is_empty()) {
            debug!("Skipping empty record on line {line_num}");
            continue;
        }

        let get = |idx: usize| -> &str { record.get(idx).unwrap_or("").trim() };

        let parse_cents = |idx: usize, field: &str| -> Result<i64> {
            let raw = get(idx);
            if raw.is_empty() {
                return Ok(0);
            }
            raw.parse::<i64>()
                .with_context(|| format!("Line {line_num}: invalid {field} value '{raw}'"))
        };

        let row = CardTraderSaleRow {
            id: get(idx_id).to_string(),
            order_code: get(idx_order_code).to_string(),
            sold_at: get(idx_sold_at).to_string(),
            item_name: get(idx_item_name).to_string(),
            item_expansion: get(idx_expansion).to_string(),
            item_properties: get(idx_properties).to_string(),
            amount_cents: parse_cents(idx_amount, "Amount (cents)")?,
            cancelation_cents: parse_cents(idx_cancelation, "Amount for cancelation (cents)")?,
            fee_cents: parse_cents(idx_fee, "CardTrader fee (cents)")?,
            currency: get(idx_currency).to_string(),
        };

        rows.push(row);
    }

    info!("Parsed {} rows from CardTrader sales report", rows.len());
    Ok(rows)
}

/// Consolidates sale rows into a single invoice for `recipient`.
///
/// Rows that net to zero (cancelled sales and their zero-value duplicates) are
/// excluded. Remaining rows are grouped by card name + expansion + properties,
/// so repeated identical cards become one position with a quantity.
pub fn consolidate(
    rows: &[CardTraderSaleRow],
    recipient: InvoiceRecipient,
) -> Result<ConsolidatedInvoice> {
    if rows.is_empty() {
        anyhow::bail!("CardTrader report contains no rows");
    }

    let billable: Vec<&CardTraderSaleRow> = rows.iter().filter(|r| !r.is_void()).collect();
    let skipped_row_count = rows.len() - billable.len();

    if skipped_row_count > 0 {
        info!("Skipping {skipped_row_count} cancelled or zero-value rows");
    }

    if billable.is_empty() {
        anyhow::bail!("CardTrader report contains no billable rows (all cancelled or zero-value)");
    }

    // Group while preserving first-seen order, so the invoice mirrors the report.
    let mut positions: Vec<ConsolidatedPosition> = Vec::new();
    let mut key_to_index: std::collections::HashMap<(String, String, String), usize> =
        std::collections::HashMap::new();

    for row in &billable {
        let key = row.grouping_key();
        match key_to_index.get(&key) {
            Some(&idx) => {
                let position: &mut ConsolidatedPosition = &mut positions[idx];
                position.quantity += 1;
                position.total_cents += row.net_cents();
            }
            None => {
                key_to_index.insert(key, positions.len());
                positions.push(ConsolidatedPosition {
                    item_name: row.item_name.clone(),
                    item_expansion: row.item_expansion.clone(),
                    item_properties: row.item_properties.clone(),
                    quantity: 1,
                    total_cents: row.net_cents(),
                });
            }
        }
    }

    let currency = resolve_currency(&billable);
    let (first_date, last_date) = date_range(&billable);

    let period_label = if first_date == last_date {
        first_date.clone()
    } else {
        format!("{first_date} - {last_date}")
    };

    let invoice = ConsolidatedInvoice {
        recipient,
        invoice_date: last_date,
        period_label,
        positions,
        currency,
        row_count: billable.len(),
        skipped_row_count,
    };

    info!(
        "Consolidated {} rows into {} positions, net total {:.2} {}",
        invoice.row_count,
        invoice.positions.len(),
        invoice.total(),
        invoice.currency
    );

    Ok(invoice)
}

/// Determines the report currency, warning if the report mixes currencies.
fn resolve_currency(rows: &[&CardTraderSaleRow]) -> String {
    let first = rows
        .iter()
        .map(|r| r.currency.as_str())
        .find(|c| !c.is_empty())
        .unwrap_or("EUR");

    let mixed = rows
        .iter()
        .any(|r| !r.currency.is_empty() && r.currency != first);

    if mixed {
        warn!("CardTrader report mixes currencies; invoicing all positions as {first}");
    }

    first.to_string()
}

/// Returns the earliest and latest sale date (`YYYY-MM-DD`) across the rows.
fn date_range(rows: &[&CardTraderSaleRow]) -> (String, String) {
    let mut dates: Vec<&str> = rows
        .iter()
        .filter_map(|r| sale_date(&r.sold_at))
        .collect::<Vec<_>>();
    dates.sort_unstable();

    match (dates.first(), dates.last()) {
        (Some(first), Some(last)) => (first.to_string(), last.to_string()),
        _ => {
            warn!("CardTrader report contains no parsable sale dates");
            (String::new(), String::new())
        }
    }
}

/// Extracts the `YYYY-MM-DD` date portion from a `Sold at (datetime)` value.
///
/// Input looks like `2026-07-01 00:01:09 +0200`.
fn sale_date(sold_at: &str) -> Option<&str> {
    let date = sold_at.split_whitespace().next()?;
    if date.len() == 10 && date.as_bytes()[4] == b'-' && date.as_bytes()[7] == b'-' {
        Some(date)
    } else {
        None
    }
}

/// Validates a consolidated invoice before it is sent to SevDesk.
///
/// Returns a list of error messages; empty when the invoice is valid.
pub fn validate_consolidated(invoice: &ConsolidatedInvoice) -> Vec<String> {
    let mut errors = invoice.recipient.validate();

    if invoice.positions.is_empty() {
        errors.push("Invoice has no positions".to_string());
    }

    if invoice.total_cents() <= 0 {
        errors.push(format!(
            "Invoice net total must be positive, got {:.2} {}",
            invoice.total(),
            invoice.currency
        ));
    }

    if invoice.invoice_date.is_empty() {
        errors.push("Invoice date could not be determined from the report".to_string());
    }

    if invoice.currency.trim().is_empty() {
        errors.push("Invoice currency is empty".to_string());
    }

    for position in &invoice.positions {
        if position.total_cents < 0 {
            errors.push(format!(
                "Position '{}' has a negative total ({:.2})",
                position.display_name(),
                position.total_price()
            ));
        }
    }

    errors
}

#[cfg(test)]
#[path = "cardtrader_parser_tests.rs"]
mod tests;
