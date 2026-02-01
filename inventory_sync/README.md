# Inventory Sync

MTG inventory sync server that collects historical pricing data and syncs stock from CSV exports to a SQLite database.

## Status

🚧 **Under Development** - Core price collection implemented.

## Current Features

- **Cardmarket Product Catalog**: Fetches full MTG product catalog (singles + non-singles, ~120k products)
- **Cardmarket Price Guide**: Fetches daily price data with trend, avg, low prices (normal + foil variants)
- **Historical Price Storage**: Stores one price snapshot per product per day (never overwrites historical data)
- **SQLite Database**: Persistent storage with products and price_history tables
- **CLI Configuration**: Customizable database path via `--database` flag

## Database Schema

### Products Table
Stores product catalog information (id, name, category, expansion, etc.)

### Price History Table
Stores daily price snapshots with composite key (id_product, price_date):
- `avg`, `low`, `trend` - regular card prices
- `avg1`, `avg7`, `avg30` - 1-day, 7-day, 30-day averages
- `avg_foil`, `low_foil`, `trend_foil` - foil variant prices
- `created_at` - Cardmarket's price guide creation timestamp

## Usage

```bash
# Run with default database (~/.local/share/inventory_sync/inventory.db)
cargo run

# Run with custom database path
cargo run -- --database /path/to/inventory.db

# With debug logging
RUST_LOG=debug cargo run

# Query price history with product names
sqlite3 inventory.db "
  SELECT p.name, ph.price_date, ph.trend, ph.avg
  FROM price_history ph
  JOIN products p ON ph.id_product = p.id_product
  WHERE p.name LIKE '%Black Lotus%'
  ORDER BY ph.price_date DESC;
"
```

## Planned Features

- **REST API**: HTTP endpoints for card sync and price queries
- **Scheduled Jobs**: Background price collection (daily cron)

## Docker Deployment

### Build and Run

```bash
# Build the Docker image
docker build -t inventory_sync .

# Run once (data persists in named volume)
docker run --rm -v inventory_data:/data inventory_sync

# Run with debug logging
docker run --rm -e RUST_LOG=debug -v inventory_data:/data inventory_sync

# Using docker compose
docker compose run --rm inventory_sync
```

### Pull from GitHub Container Registry

```bash
# Pull latest release
docker pull ghcr.io/YOUR_USERNAME/inventory_sync:latest

# Run from registry
docker run --rm -v inventory_data:/data ghcr.io/YOUR_USERNAME/inventory_sync:latest
```

### Daily Scheduling with Cron

Add to your crontab (`crontab -e`) to run daily at 3 AM:

```bash
0 3 * * * docker run --rm -v inventory_data:/data ghcr.io/YOUR_USERNAME/inventory_sync:latest >> /var/log/inventory_sync.log 2>&1
```

### Access the Database

```bash
# Copy database from volume to host
docker run --rm -v inventory_data:/data -v $(pwd):/out alpine cp /data/inventory.db /out/

# Or use sqlite3 directly in the container
docker run --rm -v inventory_data:/data -it alpine sh -c "apk add sqlite && sqlite3 /data/inventory.db"
```

## Development

```bash
# Run quality checks
./run_quality_checks.sh

# Run tests
cargo test

# Run tests in Docker (same as CI)
docker build --target tester -t inventory_sync:test .
docker run --rm inventory_sync:test

# Build release
cargo build --release
```

## Security

- All database queries use parameterized statements (no SQL string concatenation)
- All write operations are wrapped in transactions for atomicity
- Input validation on all external data (API responses)

## License

Private - All rights reserved
