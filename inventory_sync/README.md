# Inventory Sync

REST API server that collects daily MTG pricing data from Cardmarket and stores it in SQLite. Includes a web UI for card search, price charts, and card images.

## Running

```bash
cd inventory_sync
cargo run -- --web-port 3000
# Then open http://localhost:3000
```

### CLI Options

| Flag | Default | Description |
|------|---------|-------------|
| `--database PATH` | `~/.local/share/inventory_sync/inventory.db` | Database location |
| `--web-port PORT` | (disabled) | Enable web UI on this port |
| `--once` | false | Sync once and exit |
| `--interval-hours N` | 1 | Hours between sync cycles |

## API

All endpoints return a `{"success": …, "data": …, "error": …}` envelope. The
wire types and a typed client live in `mtg_common::inventory_sync` and are
used by the `check_stock` desktop app.

- `GET /api/health` — connectivity check
- `GET /api/search?q={query}&limit={n}` — product search by name
- `GET /api/prices/{id}?days=90` — one product's price history plus
  server-computed indicators and Cardmarket signals
- `POST /api/latest-prices` (`{"ids": […]}`, max 10 000) — most recent price
  row per product
- `POST /api/price-snapshots` (`{"ids": […], "dates": ["YYYY-MM-DD", …]}`,
  max 10 000 IDs × 8 dates) — the price row in effect on each requested date
  (most recent row on or before it). Deliberately a pure indexed lookup: no
  aggregation happens server-side; clients (check_stock's Price Movers and
  Mispricing screens) compute the 7/30-day deltas locally.
- `GET /api/card-image/{id}` — cached card image (via Scryfall)
- `GET /api/card-info/{id}` — cached Scryfall metadata

## Docker

```bash
docker compose up
# Or manually:
docker build -t inventory_sync .
docker run -p 8080:8080 -v inventory_data:/data inventory_sync --web-port 8080
```

## Timezone

All date comparisons use **Europe/Berlin** (Cardmarket timestamps are CET/CEST).

## Development

```bash
./run_quality_checks.sh
```
