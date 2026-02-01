# Inventory Sync

MTG inventory sync server that collects pricing data and syncs stock from CSV exports to a SQLite database.

## Status

🚧 **Under Development** - Basic price guide fetching implemented.

## Current Features

- **Cardmarket Price Guide**: Fetches MTG price guide from Cardmarket CDN on startup (~120k entries)

## Planned Features

- **REST API**: HTTP endpoints for card sync and price queries
- **SQLite Database**: Persistent storage with historical price data
- **Scheduled Jobs**: Background price collection every 12 hours
- **Docker Deployment**: Containerized deployment with volume mounts

## Usage

```bash
# Run the application
cargo run

# With debug logging
RUST_LOG=debug cargo run
```

## Development

```bash
# Run quality checks
./run_quality_checks.sh

# Run tests
cargo test

# Build release
cargo build --release
```

## License

Private - All rights reserved
