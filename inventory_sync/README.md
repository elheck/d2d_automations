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
| `--web-bind IP` | `127.0.0.1` | Address the web UI listens on (env `INVENTORY_SYNC_WEB_BIND`). The Docker image sets `0.0.0.0` (needed inside the container); what the host publishes decides who can reach it |
| `--trusted-proxy IP/CIDR` | none | Proxy whose `X-Forwarded-For` is believed, e.g. Caddy (repeatable; env `INVENTORY_SYNC_TRUSTED_PROXIES`, comma-separated) |
| `--allowed-host NAME` | none | Host name the web UI answers to besides localhost (repeatable; env `INVENTORY_SYNC_ALLOWED_HOSTS`). Without any, the `Host` header is not checked (logged as a warning) |

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

## Safety

The tracker is open to the home LAN without login (the owner's decision: its
own page calls the API from the browser, and its data is market prices). In
return it stays read-only and protects itself:

- **Search:** at most 5,000 results (`limit` is clamped; the precon loader
  asks for exactly that) and 100 characters of search text; dates must be
  `YYYY-MM-DD`.
- **Scryfall** is only asked about products in the database. One request at
  a time, 100 ms apart. Images come only from `cards.scryfall.io` (no
  redirects, at most 4 MB, JPEG or PNG). A product Scryfall does not know is
  remembered for 30 days (`card_images/<id>.miss`); a Scryfall outage is not.
- **Rate limits per client:** 600 requests per minute (bursts of 120, enough
  for a search page with 100 thumbnails) and 60 Scryfall lookups per minute;
  over the limit: `429` with `Retry-After`. Behind a proxy, set
  `--trusted-proxy` so every client gets its own limit.
- **Errors** never quote the request or internal details; a panic becomes a
  500, a poisoned database lock is recovered, requests time out after 60 s,
  bodies are at most 512 KiB.
- **Headers on every response:** a Content Security Policy (the page's inline
  script and styles, Chart.js from jsDelivr with an integrity hash, Google
  Fonts; nothing else), `nosniff`, no framing, `no-referrer`,
  `Cache-Control: no-store` (images: one day).
- **Reachability:** loopback unless `--web-bind` says otherwise; with
  `--allowed-host` the `Host` header is checked against DNS rebinding.

## Docker

```bash
docker compose up
# Or manually:
docker build -t inventory_sync .
docker run -p 8080:8080 -v inventory_data:/data inventory_sync --web-port 8080
# The image listens on 0.0.0.0 inside the container (INVENTORY_SYNC_WEB_BIND);
# publish the port only where the tracker should be reachable.
```

## Timezone

All date comparisons use **Europe/Berlin** (Cardmarket timestamps are CET/CEST).

## Development

```bash
./run_quality_checks.sh
```
