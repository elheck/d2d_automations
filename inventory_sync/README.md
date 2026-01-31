# Inventory Sync

MTG inventory sync application that collects pricing data and syncs stock from CSV exports to a SQLite database.

## Features

- **CSV Import**: Sync inventory from Cardmarket CSV exports
- **SQLite Database**: Persistent storage with full-text search indexes
- **Price Tracking**: Collect and store historical pricing data from Scryfall
- **Daemon Mode**: Run as a background service with scheduled syncs

## Usage

### Sync from CSV

```bash
cargo run -- sync path/to/export.csv
```

### Collect Prices

```bash
cargo run -- prices
```

### View Statistics

```bash
cargo run -- stats
```

### Run as Daemon

```bash
cargo run -- daemon path/to/export.csv --interval 6
```

This will sync from the CSV and collect prices every 6 hours.

## Database Schema

### Cards Table

| Column | Type | Description |
|--------|------|-------------|
| cardmarket_id | TEXT | Primary key from Cardmarket |
| name | TEXT | Card name |
| set_code | TEXT | Set code (e.g., "lea") |
| collector_number | TEXT | Collector number |
| condition | TEXT | Card condition |
| language | TEXT | Card language |
| is_foil | INTEGER | Foil flag |
| is_signed | INTEGER | Signed flag |
| quantity | INTEGER | Stock quantity |
| price | REAL | Listed price |
| location | TEXT | Storage location |
| last_synced | TEXT | Last sync timestamp |

### Price History Table

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER | Auto-increment primary key |
| cardmarket_id | TEXT | Foreign key to cards |
| price | REAL | Price at this point |
| recorded_at | TEXT | Timestamp |
| source | TEXT | Price source (e.g., "scryfall") |

## Development

```bash
# Run quality checks
./run_quality_checks.sh

# Run tests
cargo test

# Build release
cargo build --release
```

## Configuration

The application uses the following environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| RUST_LOG | Log level | info |

## License

Private - All rights reserved
