# Copilot Instructions for d2d_automations

This monorepo contains two Rust GUI applications for Magic: The Gathering business operations.

## Repository Structure

- **`check_stock/`** - MTG Stock Checker & Analysis (egui desktop app)
- **`accounting/`** - SevDesk Invoice Creator (egui desktop app, Cardmarket → SevDesk integration)

Each project is a standalone Cargo project with its own `Cargo.toml`. Run commands from within the respective directory.

## Development Workflow

```bash
# Quality checks (runs fmt, clippy, tests)
./run_quality_checks.sh

# CI-equivalent commands
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --verbose
```

Tests use fixtures in `tests/fixtures/` - prefer reusing existing fixtures over creating new test data.

## Architecture Patterns

### GUI Layer (egui/eframe)
- Main app struct implements `eframe::App` with `update()` method
- State management via dedicated state structs (e.g., `AppState`, `StockAnalysisState`)
- Screens are separate modules in `ui/screens/`, components in `ui/components/`
- See [check_stock/src/ui/app.rs](check_stock/src/ui/app.rs) for screen routing pattern

### Module Organization
- `lib.rs` re-exports all public modules for integration testing
- Inline unit tests use `_tests.rs` suffix files (e.g., `validator.rs` → `validator_tests.rs`)
- Integration tests in `tests/` directory

### Error Handling
- `check_stock`: Custom `ApiError` enum in [error.rs](check_stock/src/error.rs) with `ApiResult<T>` alias
- `accounting`: Uses `anyhow::Result` with `.context()` for error messages

### Async Pattern (accounting)
- Tokio runtime created in app state: `runtime: Runtime::new()`
- Block on async calls in GUI: `self.runtime.block_on(async_fn())`

## Domain-Specific Conventions

### CSV Parsing
- Cardmarket exports use semicolon separators (`;`)
- Multi-item orders: fields delimited with ` | ` (space-pipe-space)
- Price parsing handles both `.` and `,` as decimal separators

### Card Matching (check_stock)
- Cards have localized names: `name_de`, `name_es`, `name_fr`, `name_it`
- `Language` enum with `from_code()`, `from_full_name()`, `parse()` methods
- Location format: `A1_S1_R1_C1` (Aisle_Shelf_Row_Column)

### SevDesk API (accounting)
- API token from `SEVDESK_API` env var
- Country cache with thread-safe `Arc<RwLock<CountryCache>>`
- Dry-run mode simulates operations without API calls

## Key Files for Reference

| Pattern | Example File |
|---------|-------------|
| Screen component | `check_stock/src/ui/screens/stock_checker.rs` |
| API client | `accounting/src/sevdesk_api/client.rs` |
| CSV processor | `accounting/src/csv_processor/mod.rs` |
| Error types | `check_stock/src/error.rs` |
| Test fixtures | `accounting/tests/fixtures/*.csv` |
