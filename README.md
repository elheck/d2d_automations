# d2d_automations

[![Rust CI](https://github.com/elheck/d2d_automations/workflows/Rust%20CI/badge.svg)](https://github.com/elheck/d2d_automations/actions/workflows/rust.yml)
[![Release](https://github.com/elheck/d2d_automations/workflows/Release/badge.svg)](https://github.com/elheck/d2d_automations/actions/workflows/release.yml)

Rust monorepo for Magic: The Gathering business operations.

## Projects

| Project | Type | Description |
|---------|------|-------------|
| [check_stock](check_stock/) | egui desktop app | Stock checker, analysis, picking, pricing |
| [accounting](accounting/) | egui desktop app | Cardmarket CSV → SevDesk invoicing |
| [inventory_sync](inventory_sync/) | REST API server | CSV → SQLite with price tracking & web UI |
| [mtg_common](mtg_common/) | shared library | Common types (Cardmarket, Scryfall, errors) |

## Quick Start

```bash
cd <project>
cargo run            # desktop apps
cargo run -- --web-port 3000  # inventory_sync server
```

`accounting` requires `SEVDESK_API` env var.

## Development

```bash
cd <project>
./run_quality_checks.sh  # fmt, clippy, tests, build, audit
```

## Review Plan

Full security and refactoring review conducted 2026-03-15. Findings below, ordered by priority.

### Phase 1: Critical Security (inventory_sync)

| # | Issue | Risk |
|---|-------|------|
| C1 | No authentication on any endpoint | Full data exposure |
| C2 | Binds to `0.0.0.0` instead of `127.0.0.1` | Network exposure |
| C3 | No rate limiting | DoS, Scryfall API abuse |

### Phase 2: High Security

| # | Project | Issue | Risk |
|---|---------|-------|------|
| H1 | inventory_sync | `Mutex::lock().unwrap()` — poison = permanent crash | Permanent DoS |
| H2 | inventory_sync | Error details leaked to clients | Info disclosure |
| H3 | inventory_sync | Unbounded `limit` query parameter | Memory exhaustion |
| H4 | inventory_sync | No CORS configuration | Future CSRF risk |
| H5 | inventory_sync | Docker port exposed on all interfaces | Network exposure |
| H6 | accounting | Path traversal in PDF filename via `invoice_number` | Arbitrary file write |
| H7 | accounting | Full API response bodies logged (PII, IBANs) | Data leak via logs |
| H8 | accounting | Fuzzy substring country matching (Niger↔Nigeria) | Wrong invoices |
| H9 | accounting | Silent success on failed invoice position creation | Missing line items |

### Phase 3: Medium Security

| # | Project | Issue |
|---|---------|-------|
| M1 | check_stock | SSRF via unvalidated image URL from Scryfall |
| M2 | check_stock | Path traversal in cache filenames |
| M3 | check_stock | URL injection via unsanitized set_code/collector_number |
| M4 | accounting | API token stored as plain mutable String |
| M5 | accounting | Token length logged at debug level |
| M6 | inventory_sync | LIKE metacharacter injection in search |
| M7 | inventory_sync | No graceful shutdown |
| M8 | inventory_sync | Unbounded image cache can fill disk |
| M9 | inventory_sync | CDN script without SRI hash |

### Phase 4: Refactoring — Within Projects

| # | Issue | Project |
|---|-------|---------|
| R1 | 8x duplicated `create_test_card` helper | check_stock |
| R2 | Duplicated `parse_price` function | accounting |
| R3 | `InvoiceApp` god object (740 lines) | accounting |
| R4 | 40x `#[allow(dead_code)]` annotations | all |
| R5 | `partial_cmp().unwrap()` on f64 (panics on NaN) | check_stock |
| R6 | No HTTP timeout on reqwest clients | accounting, check_stock |