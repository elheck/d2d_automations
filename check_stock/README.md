# MTG Stock Checker

egui desktop app for Magic: The Gathering card inventory management.

## Screens

- **Stock Checker** — Match inventory CSV against wantslists
- **Stock Analysis** — Inventory overview, sales metrics, **sales velocity**
  (copies/revenue per week from daily snapshots), **dead-stock aging** (in-stock
  cards bucketed by how long they've been listed), and a **Lot Cost & Margin**
  table: per-lot revenue, remaining stock value, and — once you record a lot's
  acquisition cost — realized margin % and payback status. Click a Cost cell to
  enter or correct a lot's purchase price; the figure is saved to the inventory DB
  (`lot_costs` table) and can be edited or cleared at any time.
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
- **Search Cards** — Interactive inventory search with filtering. Each result row
  has a **price-history button (📈)** that opens a floating window with the card's
  trend-price sparkline and 7/30-day movement, fetched from the inventory_sync
  server (foil-aware). Selected cards
  can either be sent to the Stock Checker lists, or **discarded**: choose the
  "Discard (remove without affecting revenue)" action to write cards off as junk.
  This reduces the inventory DB *without* counting them as sold (tracked revenue is
  unaffected) and exports a negative-delta stock-update CSV. Import that CSV into
  Cardmarket before your next inventory sync so the drop is already reflected in
  both places and no phantom sale is recorded.
- **Picking** — Order picking workflow (reached via Stock Checker results)
- **Pricing** — Node-based visual editor for filtering and pricing stock from CSV inventory
- **Card Buy Helper** — Value a purchase offer from a card export CSV: split cards into individually-priced singles (by rarity and/or price threshold) versus bulk (flat rate per N cards), see the total offer, and export a breakdown CSV. Strictly read-only — never writes to the inventory database.
- **Mispricing Report** — Compare every in-stock listing against a market
  reference (trend/avg/low, foil-aware) to surface under- and over-priced
  cards, the revenue upside of repricing, and capital stuck above market. The
  market source is either the **inventory_sync server** (default — latest
  collected prices, no big download, plus **Δ7d/Δ30d market-movement columns**)
  or a full Cardmarket price-guide download / local JSON file. Each row is
  enriched with market signals combined into an **Action column** ("Raise now",
  "Cut now", "Hold", …): avg1/avg7/avg30 **momentum** crossed with the verdict,
  listings **below the market low** escalated to underpriced, the fair band
  **widened by the card's own ~90-day volatility** (a delta inside its natural
  swing is noise), **stale market data** (> 7 days) flagged and de-weighted,
  and **listing age** boosting old overpriced stock (dead capital). The default
  sort is a transparent **priority score** (impact × urgency × confidence) —
  what to fix first is on top. Strictly read-only — it reports, it never writes
  prices.
- **Restock Report** — Sold-out variants (quantity 0, copies sold > 0) ranked by
  sell-through speed (copies/week over the listing → last-sale window), with a
  minimum-copies filter to hide one-off sales and a buy-list CSV export. Answers
  "what should I buy again?". Backed by the per-variant **sold-events log**: every
  inventory sync records which variant sold how many copies on what date and at
  what listed price (discards/write-offs never count as sales). Strictly read-only.
- **Price Movers** — Joins the in-stock inventory with 7/30-day market movement
  from inventory_sync price snapshots: spikes worth selling into and falling
  knives worth liquidating, filterable by direction, minimum price, and minimum
  listing age (old stock that is also losing value is the first liquidation
  candidate). All deltas are computed locally from raw snapshot rows — the
  server only runs indexed lookups. Strictly read-only.

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
- **inventory_sync server**: Latest collected prices, raw price snapshots for
  7/30-day movement, and per-card price history (see `inventory_sync/`; the
  server URL is configured once in the shared connection bar and used by the
  Pricing, Mispricing, Price Movers and Search screens)

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
