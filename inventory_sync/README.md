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
