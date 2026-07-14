# Inventory Sync

REST API server that collects daily MTG pricing data from Cardmarket and stores it in SQLite. Includes a web UI for card search, price charts, card images, and a **Buy Signals** view that ranks the best cards to buy right now.

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

## Buy Signals

The web UI has a **💎 Buy Signals** tab that ranks the best *undervalued dip-buys* —
cards trading below their own trend and history and therefore likely to recover
toward trend. Each card gets a 0–100 buy score combining several signals:

| Signal | What it looks for |
|--------|-------------------|
| Floor ratio (`low ÷ trend`) | Listings undercutting the trend price. Peaks around a healthy 25% undercut; implausible sub-30% ratios are treated as noise (single mispriced/damaged copies) and discarded. |
| RSI | Oversold cards (RSI < 30) that have been sold down and may bounce. |
| Bollinger %B | Price low in its own historical range. |
| 30-day ROC | Cards that have dropped recently — the "dip". |
| 1d/7d momentum | Short-term price turning back up (the start of a bounce). |

A floor undercut on its own is discounted unless a technical signal (oversold RSI,
low band position, or a recent dip) corroborates it, so noisy one-off listings don't
dominate the ranking. Sub-€1 penny cards are filtered out by default.

The ranking is **precomputed once per day**, right after new price data is ingested,
and stored in the `buy_signals` table so the view loads instantly. If it has never
run yet (e.g. right after first startup), it is computed lazily on the first request.

### API

- `GET /api/buy-signals?limit=100` — ranked candidates plus scan metadata
  (`price_date`, `computed_at`). `limit` is capped at 500.

Clicking a candidate jumps to its price chart in the Search view.

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
