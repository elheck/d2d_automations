# SevDesk Invoice Creator

egui desktop app for creating SevDesk invoices from Cardmarket CSV order exports.

## What It Does

- Loads Cardmarket order CSV exports (semicolon-separated, multi-item orders supported)
- Creates SevDesk contacts, invoices, and line items via REST API
- Full invoice workflow: finalize, send, enshrine, book, PDF download
- Check account selection for booking
- Dry-run mode for testing without API side effects
- Kleingewerbe tax rules (0% VAT, section 19 UStG)

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
