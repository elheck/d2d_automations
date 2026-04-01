# MTG Stock Checker

egui desktop app for Magic: The Gathering card inventory management.

## Screens

- **Stock Checker** — Match inventory CSV against wantslists, multi-language card matching
- **Stock Analysis** — Bin capacity utilization, free-slot analysis, and per-lot revenue tracking
- **Magic Singles Listing** — Card lookup via Scryfall by set code + collector number, with images and Cardmarket prices
- **Search Cards** — Interactive inventory search with filtering
- **Picking** — Order picking workflow (reached via Stock Checker results)
- **Pricing** — Node-based visual editor for filtering and pricing stock from CSV inventory

## Data Sources

- **Inventory**: Cardmarket CSV stock export (semicolon-separated)
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
