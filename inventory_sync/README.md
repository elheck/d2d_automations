# Inventory Sync

MTG inventory sync application that will collect pricing data and sync stock from CSV exports to a SQLite database.

## Status

🚧 **Under Development** - This project is currently a skeleton with no implemented features yet.

## Planned Features

- **CSV Import**: Sync inventory from Cardmarket CSV exports
- **SQLite Database**: Persistent storage with full-text search indexes
- **Price Tracking**: Collect and store historical pricing data from Scryfall
- **Daemon Mode**: Run as a background service with scheduled syncs

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
