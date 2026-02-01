# Copilot Instructions for d2d_automations

Rust monorepo for Magic: The Gathering business operations.

## Projects

| Project | Type | Status |
|---------|------|--------|
| `check_stock/` | egui desktop app - Stock Checker & Analysis | Active |
| `accounting/` | egui desktop app - Cardmarket → SevDesk invoicing | Active |
| `inventory_sync/` | REST API server - CSV → SQLite with price tracking | Skeleton |

Each project is standalone with its own `Cargo.toml`. Run commands from within the project directory.

## Development Workflow

```bash
cd <project>
./run_quality_checks.sh  # Runs fmt, clippy, tests - use this for consistency with CI
```

Tests use fixtures in `tests/fixtures/` - prefer reusing existing fixtures over creating new test data.

## Post-Change Checklist

1. **Update README files**: Search all `README.md` files and update affected sections
2. **Check FEATURE_REQUESTS**: Mark completed features with implementation date in `FEATURE_REQUESTS.md` files
3. **Unit test coverage**: All new public functions/structs need tests. Run `./run_quality_checks.sh`

## Security Guidelines

**Security > Performance.** Be unnecessarily secure - expect a penetration test at any second.

- **Parameterized queries only**: Never string-concat SQL. Always use prepared statements.
- **Input validation**: Validate all external input (user input, API responses, CSV files).
- **No secrets in code**: Use environment variables (`SEVDESK_API`, etc.). Never hardcode.
- **Communicate risks**: Before security-impacting changes, explain risks and mitigations. Wait for acknowledgment.

### Server-Specific (inventory_sync)
- Atomic/transactional database writes for safe shutdown
- API authentication required on all endpoints
- Bind to `127.0.0.1` by default; require explicit config for external exposure
- Rate limiting to prevent abuse; never expose stack traces to clients

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

## Key Files

| Pattern | Example |
|---------|----------|
| Screen component | `check_stock/src/ui/screens/stock_checker.rs` |
| Screen state | `check_stock/src/ui/screens/picking.rs` (PickingState) |
| API client | `accounting/src/sevdesk_api/client.rs` |
| CSV processor | `accounting/src/csv_processor/mod.rs` |
| Error types | `check_stock/src/error.rs` |
| Unit tests | `accounting/src/csv_processor/validator_tests.rs` |
| Integration tests | `accounting/tests/csv_processor_tests.rs` |
| Test fixtures | `accounting/tests/fixtures/*.csv` |
