# MTG Stock Checker

egui desktop app for Magic: The Gathering card inventory management.

## Screens

- **Stock Checker** — Match inventory CSV against wantslists
- **Stock Analysis** — Inventory overview, per-lot revenue tracking, sales metrics,
  **sales velocity** (copies/revenue per week from daily snapshots), and **dead-stock
  aging** (in-stock cards bucketed by how long they've been listed)
- **Bin Analysis** — Bin capacity utilization and free-slot analysis, plus two
  consolidation tools:
  - **Consolidation suggestions** — empties sparse bins into fuller ones (preferring
    bins that already hold the same card, then closest by proximity). The planner
    tries several source orderings and keeps the best plan (most bins freed, then
    least walking).
  - **Fragmented-variant report** — finds card variants scattered across multiple
    bins regardless of fill and gathers each into a single bin.

  Both open an **interactive move list** (card-image tiles grouped by source bin,
  with a "Mark Moved" toggle and progress bar) for all piles or a single chosen
  source bin. The CSV export lives in that list and includes **only the piles you
  tick as moved**, so it reflects what you actually did. Read-only — never writes to
  the inventory DB; moves apply when you re-load an updated CSV, and each card keeps
  its lot/side so per-lot revenue is unaffected.
- **Magic Singles Listing** — Card lookup via Scryfall by set code + collector number, with images and Cardmarket prices
- **Search Cards** — Interactive inventory search with filtering. Selected cards
  can either be sent to the Stock Checker lists, or **discarded**: choose the
  "Discard (remove without affecting revenue)" action to write cards off as junk.
  This reduces the inventory DB *without* counting them as sold (tracked revenue is
  unaffected) and exports a negative-delta stock-update CSV. Import that CSV into
  Cardmarket before your next inventory sync so the drop is already reflected in
  both places and no phantom sale is recorded.
- **Picking** — Order picking workflow (reached via Stock Checker results)
- **Pricing** — Node-based visual editor for filtering and pricing stock from CSV inventory
- **Card Buy Helper** — Value a purchase offer from a card export CSV: split cards into individually-priced singles (by rarity and/or price threshold) versus bulk (flat rate per N cards), see the total offer, and export a breakdown CSV. Strictly read-only — never writes to the inventory database.
- **Mispricing Report** — Compare every in-stock listing against a Cardmarket
  price-guide reference (trend/avg/low, foil-aware) to surface under- and over-priced
  cards, the revenue upside of repricing, and capital stuck above market. Fetches the
  price guide from the Cardmarket CDN or loads it from a local JSON file. Strictly
  read-only — it reports, it never writes prices.

## Data Sources

- **Inventory**: Cardmarket *inventory-report* CSV (comma-separated). Legacy *export* CSVs
  (with `nameDE`/`nameES`/`nameFR`/`nameIT` / `listedAt` columns) also load. Condition values
  may be either short codes (`NM`, `EX`, `GD`, `LP`, `PL`) or the inventory-report long form
  (`near_mint`, `excellent`, `good`, `light_played`, `played`, `poor`).
- **Wantslists / decklists**: `quantity name` text, plus the common deck-export
  formats — MTG Arena, MTGO, Moxfield, Archidekt and MTGGoldfish. Set codes,
  collector numbers, foil/etched markers (`*F*`/`*E*`), category `[…]` and tag
  `^…^` annotations, `SB:` sideboard prefixes and section headers are handled;
  `4x`/`4X` quantities are accepted; duplicate card names are merged. The
  Wantslist field also accepts a pasted **Moxfield or Archidekt deck link**
  (e.g. `https://moxfield.com/decks/<id>`), which is fetched over the network.
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
