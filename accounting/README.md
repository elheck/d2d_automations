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
- Kleingewerbe tax rules (0% VAT, section 19 UStG)

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
