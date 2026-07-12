# MTG Stock Checker

egui desktop app for Magic: The Gathering card inventory management.

## Screens

- **Stock Checker** — Match inventory CSV against wantslists
- **Stock Analysis** — Inventory overview, per-lot revenue tracking, and sales metrics
- **Bin Analysis** — Bin capacity utilization and free-slot analysis
- **Magic Singles Listing** — Card lookup via Scryfall by set code + collector number, with images and Cardmarket prices
- **Search Cards** — Interactive inventory search with filtering
- **Picking** — Order picking workflow (reached via Stock Checker results)
- **Pricing** — Node-based visual editor for filtering and pricing stock from CSV inventory
- **Card Buy Helper** — Value a purchase offer from a card export CSV: split cards into individually-priced singles (by rarity and/or price threshold) versus bulk (flat rate per N cards), see the total offer, and export a breakdown CSV. Strictly read-only — never writes to the inventory database.

## Data Sources

- **Inventory**: Cardmarket *inventory-report* CSV (comma-separated). Legacy *export* CSVs
  (with `nameDE`/`nameES`/`nameFR`/`nameIT` / `listedAt` columns) also load. Condition values
  may be either short codes (`NM`, `EX`, `GD`, `LP`, `PL`) or the inventory-report long form
  (`near_mint`, `excellent`, `good`, `light_played`, `played`, `poor`).
- **Wantslists**: Simple `quantity name` text format
- **Scryfall API**: Card data, images
- **Cardmarket CDN**: Price guide (~50MB, all MTG products)

## Caching

Card data and images are cached locally in the platform cache directory (Linux: `~/.cache/d2d_automations/`). Local SQLite database for inventory sync.

## Running

```bash
cd check_stock
cargo run
```

## Development

```bash
./run_quality_checks.sh
```
