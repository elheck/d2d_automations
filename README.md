# d2d_automations

[![Rust CI](https://github.com/elheck/d2d_automations/workflows/Rust%20CI/badge.svg)](https://github.com/elheck/d2d_automations/actions/workflows/rust.yml)
[![Release](https://github.com/elheck/d2d_automations/workflows/Release/badge.svg)](https://github.com/elheck/d2d_automations/actions/workflows/release.yml)

## Overview

d2d_automations is a monorepo containing Rust applications for Magic: The Gathering business operations.

## Projects

| Project | Description | Status |
|---------|-------------|--------|
| [check_stock](check_stock/) | MTG Stock Checker & Analysis (egui desktop app) | ✅ Active |
| [accounting](accounting/) | SevDesk Invoice Creator (egui desktop app, Cardmarket → SevDesk) | ✅ Active |
| [inventory_sync](inventory_sync/) | Inventory Sync (CLI app, CSV → SQLite with price tracking) | 🚧 Planned |

## Quick Start

Each project is a standalone Cargo project. Navigate to the respective directory:

```bash
# Check Stock (desktop GUI)
cd check_stock
cargo run

# Accounting (desktop GUI)
cd accounting
export SEVDESK_API="your_token_here"
cargo run

# Inventory Sync (CLI)
cd inventory_sync
cargo run
```

## Development

### Quality Checks

Each project has its own quality check script:

```bash
cd <project>
./run_quality_checks.sh
```

This runs:
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --verbose`

### CI/CD

- **Continuous Integration**: GitHub Actions runs on push/PR to main
- **Release Builds**: Triggered by tags (e.g., `v1.0.0`)

## Testing

```bash
# Run tests for a specific project
cd check_stock
cargo test

# Run performance tests
cargo test test_search_performance -- --nocapture
```

For detailed information about performance testing, see [check_stock/PERFORMANCE_TESTING.md](check_stock/PERFORMANCE_TESTING.md).

## Building for Release

```bash
cd <project>
cargo build --release
```

## Troubleshooting

- If you encounter build errors, ensure you have the latest stable Rust toolchain: `rustup update`
- For platform-specific issues, check the workflow logs in the GitHub Actions tab.

## License

MIT License. See `LICENSE` file for details.