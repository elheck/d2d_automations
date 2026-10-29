# d2d_automations

[![Rust CI](https://github.com/elheck/d2d_automations/workflows/Rust%20CI/badge.svg)](https://github.com/elheck/d2d_automations/actions/workflows/rust.yml)
[![Release](https://github.com/elheck/d2d_automations/workflows/Release/badge.svg)](https://github.com/elheck/d2d_automations/actions/workflows/release.yml)

Rust monorepo for Magic: The Gathering business operations.

## Projects

| Project | Type | Description |
|---------|------|-------------|
| [check_stock](check_stock/) | egui desktop app | Stock checker, analysis, picking, pricing, precon EV |
| [accounting](accounting/) | egui desktop app | Cardmarket CSV & CardTrader reports → SevDesk invoicing |
| [inventory_sync](inventory_sync/) | REST API server | CSV → SQLite with price tracking & web UI |
| [mtg_common](mtg_common/) | shared library | Shared types, API clients (Cardmarket, Scryfall, inventory_sync, MTGJSON), file cache, errors |

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

`./run_quality_checks.sh` in the repo root runs every project's checks plus
`./check_release_targets.sh`.

### Release platforms (macOS / Windows)

Some compile errors only happen on the macOS or Windows release builds (e.g.
a dependency's macOS-only code). Three ways to catch them before tagging a
release:

- **Locally**: `./check_release_targets.sh [project]` runs `cargo check` for
  the release targets from Linux (installs the Rust targets on first use).
  Crates with C code (`ring`, bundled SQLite) need the platform SDK and are
  skipped together with everything depending on them; the rest, including the
  platform-specific GUI code, is checked.
- **Every push / PR**: the `release_targets` job of the Rust CI workflow builds
  and tests check_stock on macOS + Windows and accounting on Windows, with the
  same command as the release workflow.
- **The full release pipeline on demand**: run the *Release* workflow manually
  (Actions → Release → Run workflow, or `gh workflow run release.yml --ref <branch>`).
  It builds everything without publishing; uploads only happen for `v*` tags.

## Review Plan

Full security and refactoring review conducted 2026-03-15. Findings below, ordered by priority.
Status last checked 2026-10-29.

### Phase 1: Critical Security (inventory_sync)

| # | Issue | Risk | Status |
|---|-------|------|--------|
| C1 | No authentication on any endpoint | Full data exposure | Open |
| C2 | Binds to `0.0.0.0` instead of `127.0.0.1` | Network exposure | Open |
| C3 | No rate limiting | DoS, Scryfall API abuse | Open |

### Phase 2: High Security

| # | Project | Issue | Risk | Status |
|---|---------|-------|------|--------|
| H1 | inventory_sync | `Mutex::lock().unwrap()` — poison = permanent crash | Permanent DoS | Open |
| H2 | inventory_sync | Error details leaked to clients | Info disclosure | Open (`/api/card-image` returns the error text) |
| H3 | inventory_sync | Unbounded `limit` query parameter | Memory exhaustion | Open |
| H4 | inventory_sync | No CORS configuration | Future CSRF risk | Open |
| H5 | inventory_sync | Docker port exposed on all interfaces | Network exposure | Open |
| H6 | accounting | Path traversal in PDF filename via `invoice_number` | Arbitrary file write | Open |
| H7 | accounting | Full API response bodies logged (PII, IBANs) | Data leak via logs | Open (invoice-creation body no longer logged; contact bodies still are) |
| H8 | accounting | Fuzzy substring country matching (Niger↔Nigeria) | Wrong invoices | **Fixed** — exact names only; unknown countries pause the run and the user picks the SevDesk country |
| H9 | accounting | Silent success on failed invoice position creation | Missing line items | **Fixed** — rejected line items are kept, SevDesk's total is checked against the order total, and a mismatch pauses the run before finalize/enshrine/book |

### Phase 3: Medium Security

| # | Project | Issue | Status |
|---|---------|-------|--------|
| M1 | check_stock | SSRF via unvalidated image URL from Scryfall | Open |
| M2 | check_stock | Path traversal in cache filenames | Open |
| M3 | check_stock | URL injection via unsanitized set_code/collector_number | Open |
| M4 | accounting | API token stored as plain mutable String | Open |
| M5 | accounting | Token length logged at debug level | Open |
| M6 | inventory_sync | LIKE metacharacter injection in search | Open |
| M7 | inventory_sync | No graceful shutdown | Open |
| M8 | inventory_sync | Unbounded image cache can fill disk | Open |
| M9 | inventory_sync | CDN script without SRI hash | Open |
