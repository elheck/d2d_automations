# Inventory Sync

MTG inventory sync server that collects historical pricing data and syncs stock from CSV exports to a SQLite database. Includes a modern web UI for searching cards, viewing price history, and displaying card images.

## Status

✅ **Active Development** - Core features implemented, web UI complete.

## Current Features

### Data Collection
- **Cardmarket Product Catalog**: Fetches full MTG product catalog (singles + non-singles, ~120k products)
- **Cardmarket Price Guide**: Fetches daily price data with trend, avg, low prices (normal + foil variants)
- **Historical Price Storage**: Stores one price snapshot per product per day (never overwrites historical data)
- **SQLite Database**: Persistent storage with products and price_history tables
- **Daemon Mode**: Runs continuously with configurable check intervals (default: 1 hour)
- **CLI Configuration**: Customizable database path via `--database` flag

### Web UI
- **🌐 Modern Web Interface**: Dark-themed, responsive UI built with modern CSS and Chart.js
- **🔍 Card Search**: Real-time fuzzy search across all MTG products
- **📊 Price Charts**: Interactive line charts showing trend, average, and low prices over time
- **🖼️ Card Images**: Automatic card image display from Scryfall API
- **⚡ Image Caching**: Server-side persistent cache for fast repeated image loads
- **📱 Mobile Responsive**: Works great on desktop, tablet, and mobile devices

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

### Web UI

```bash
# Run server with web UI on port 3000
cargo run -- --web-port 3000

# Run with custom database and web UI
cargo run -- --database /path/to/inventory.db --web-port 8080

# Run once (sync data then exit)
cargo run -- --once --web-port 3000

# Change check interval (default: 1 hour)
cargo run -- --interval-hours 6 --web-port 3000
```

Then open **http://localhost:3000** in your browser!

### Command-Line Only

```bash
# Run without web UI (daemon mode)
cargo run

# Run with custom database path
cargo run -- --database /path/to/inventory.db

# With debug logging
RUST_LOG=debug cargo run -- --web-port 3000
```

### Database Queries

```bash
# Query price history with product names
sqlite3 ~/.local/share/inventory_sync/inventory.db "
  SELECT p.name, ph.price_date, ph.trend, ph.avg
  FROM price_history ph
  JOIN products p ON ph.id_product = p.id_product
  WHERE p.name LIKE '%Black Lotus%'
  ORDER BY ph.price_date DESC;
"
```

## REST API Endpoints

The web server provides the following API endpoints:

- `GET /` - Web UI (single-page application)
- `GET /api/search?q={query}&limit={limit}` - Search for cards by name
- `GET /api/prices/{id_product}` - Get price history for a specific product
- `GET /api/card-image/{card_name}` - Fetch and cache card image from Scryfall

All responses are JSON with the format:
```json
{
  "success": true,
  "data": { ... },
  "error": null
}
```

## Image Cache

Card images are fetched from Scryfall's API and cached locally in:
```
~/.local/share/inventory_sync/card_images/
```

Cache features:
- **Persistent**: Images survive server restarts
- **Case-insensitive**: "Black Lotus" = "black lotus"
- **Safe filenames**: Handles special characters (`//`, `/`, etc.)
- **Automatic**: First request fetches from Scryfall, subsequent requests use cache
- **Browser caching**: 24-hour cache headers for optimal performance

## Planned Features

- **CSV Import**: Sync inventory from Cardmarket CSV exports
- **Stock Management**: Track owned inventory quantities
- **Advanced Filters**: Filter by set, rarity, price range, etc.

## Docker Deployment

### Build and Run

```bash
# Build the Docker image
docker build -t inventory_sync .

# Run with web UI (exposed on port 8080)
docker run --rm -p 8080:8080 -v inventory_data:/data inventory_sync --web-port 8080

# Run once (data persists in named volume)
docker run --rm -v inventory_data:/data inventory_sync --once

# Run with debug logging
docker run --rm -e RUST_LOG=debug -v inventory_data:/data inventory_sync

# Using docker compose
docker compose up
```

The web UI will be available at http://localhost:8080

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
# Run quality checks (fmt, clippy, tests)
./run_quality_checks.sh

# Run all tests
cargo test

# Run only unit tests
cargo test --lib

# Run integration tests (requires network access)
cargo test -- --ignored

# Run tests in Docker (same as CI)
docker build --target tester -t inventory_sync:test .
docker run --rm inventory_sync:test

# Build release
cargo build --release
```

### Test Coverage

- **Database**: 14 tests covering schema, upserts, price history, queries
- **Cardmarket API**: 2 tests for catalog and price guide parsing
- **Scryfall API**: 6 tests for card deserialization and image URLs (2 integration tests)
- **Image Cache**: 4 tests for sanitization, persistence, case-insensitivity
- **Web API**: 5 tests for router, state, serialization

Total: **32 tests** (30 unit tests + 2 integration tests)

## Security

- All database queries use parameterized statements (no SQL string concatenation)
- All write operations are wrapped in transactions for atomicity
- Input validation on all external data (API responses)

## License

Private - All rights reserved
