# SevDesk Invoice Creator

egui desktop app for creating SevDesk invoices from Cardmarket CSV order exports
and CardTrader sales reports.

## What It Does

- Loads Cardmarket order CSV exports (semicolon-separated, multi-item orders supported)
- Loads CardTrader sales reports and bills the whole report as **one** invoice
- Creates SevDesk contacts, invoices, and line items via REST API
- Full invoice workflow: finalize, send, enshrine, book, PDF download
- Check account selection for booking
- Dry-run mode for testing without API side effects
- Invoices are created in the background with a live progress bar (current order and step, success/error counts, elapsed time and ETA)
- Kleingewerbe tax rules (0% VAT, section 19 UStG)

## Checks Before Finalizing

Every invoice is created as a SevDesk **draft** first. Finalize, enshrine and
book only run once the draft is verified:

- **Line items**: if SevDesk rejects a line item, the app keeps going with the
  others and remembers the rejected ones (it no longer ignores them).
- **Total**: SevDesk's invoice total is read back and must equal the expected
  total exactly: the CSV `TotalValue` for Cardmarket orders, the report total
  for CardTrader.

If either check fails, the run **pauses** and a dialog shows the expected
total, SevDesk's total and any rejected line items. Nothing is finalized,
enshrined or booked until you pick one of:

| Option | What happens |
|--------|--------------|
| Retry failed line items | Adds the rejected items again, then checks the total again (only shown if items were rejected) |
| Check again | Re-reads the total, e.g. after you fixed the draft in SevDesk; continues if it matches |
| Keep as draft, continue | Leaves the unfinished draft in SevDesk, goes on with the next invoice |
| Delete draft, continue | Deletes the draft; the order is not invoiced |
| Stop | Keeps the draft; the remaining orders are listed as "not processed" |

The run continues with the remaining invoices after your choice (unless you
stop). A dry run reports orders whose line items don't add up to their total
as errors instead of asking.

**Countries** are matched exactly (case-insensitive) against SevDesk's German
and English country names plus a few aliases (`DE`, `UK`, `Great Britain`).
There is no partial matching and no default country. For an unknown country
the run pauses before anything is created for that order: pick the right
SevDesk country (used for every order with that name in this run), skip the
order, or stop. A dry run lists unknown countries as errors.

## CSV Formats

The format is detected automatically from the header row.

### Cardmarket order export
One invoice per order, addressed to the buyer from the CSV.

### CardTrader sales report
Recognised by the `Order Code` / `Amount (cents)` / `CardTrader fee (cents)` columns.
The entire report becomes a single consolidated invoice:

- **Net only.** Positions use `Amount` + `Amount for cancelation`. The
  `CardTrader fee` column is parsed but never billed, because CardTrader invoices
  its fees separately.
- **Cancelled rows are excluded.** A cancellation offsets its sale to zero, so
  cancelled sales and their zero-value duplicates drop out automatically.
- **Positions are grouped** by card name + expansion + properties, so repeated
  identical cards become one position with a quantity.
- **Amounts stay in integer cents** until the SevDesk API boundary, so summing
  hundreds of rows cannot accumulate floating point error.
- **The recipient is editable** in the UI and defaults to GRAY FOX SRL, Via San
  Gregorio 55, 20124 Milano, Italien. The address field only appears when a
  CardTrader report is loaded.
- The invoice/delivery date is the latest *billable* sale date in the report.

## Setup

**Environment variable:** `SEVDESK_API` (token from SevDesk Settings > API)

**Linux system dependencies:**
```bash
sudo apt-get install -y build-essential pkg-config libssl-dev \
  libfontconfig1-dev libfreetype6-dev libxcb1-dev libxcb-render0-dev \
  libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev
```

**Run:**
```bash
cd accounting
cargo run
```

## Development

```bash
./run_quality_checks.sh
```
