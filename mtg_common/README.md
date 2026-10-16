# mtg_common

Shared library for the MTG business projects in this repo (`check_stock`,
`inventory_sync`). Holds everything that used to be copy-pasted between
projects: API types, HTTP clients, and caching primitives.

## Contents

| Module | Contents |
|--------|----------|
| `scryfall` | `ScryfallCard` (superset of fields used across projects), `ImageUris`, `CardFace`, `ScryfallPrices`, `PurchaseUris`, and fetch functions (`fetch_card`, `fetch_card_by_cardmarket_id`, `fetch_card_by_name`, `fetch_image`) |
| `cardmarket` | `PriceGuide` (lookup by product ID with `load`/`fetch`), `PriceGuideEntry`, `PriceGuideFile`, and `product_url(id)` — the Cardmarket page of any product id (`Products?idProduct=…`, the same link Scryfall/MTGJSON publish; singles and sealed) |
| `inventory_sync` | Wire types (`ApiResponse`, `LatestPrice`, `PriceSnapshot`, `PriceData`, …) shared between the `inventory_sync` server (serializes) and client apps (deserialize), the `InventorySyncClient` HTTP client (`health`, `search`, `latest_prices`, `price_history`, `price_snapshots` — chunks ids and dates to the server limits), and `PriceField`/`PriceFields` — the foil-aware selector for the 12 standard Cardmarket price columns, also implemented for `PriceGuideEntry` |
| `mtgjson` | `MtgjsonClient` (`deck_list`, `sealed_index`, `deck`) for MTGJSON precon data, compact cacheable types (`PreconDeck`, `DeckCard`, `SealedIndex`, `DeckRef`), and pure helpers (`build_sealed_index`, `resolve_decks`, `parse_deck`). Responses are size-capped while streaming; deck `fileName`s must pass `is_safe_deck_file_name` before use in a URL or path |
| `file_cache` | `FileCache` — best-effort persistent byte cache backed by files in a directory; foundation for the projects' image caches |
| `error` | `MtgError` / `MtgResult` — common error type; projects convert it into their own error types via `From` |

Also exports `USER_AGENT` (shared User-Agent header for all external API
requests) and `PRICE_GUIDE_URL`.

## Features

- `blocking` — enables blocking (non-async) variants of the HTTP clients
  (`scryfall::blocking::*`, `PriceGuide::fetch_blocking`,
  `InventorySyncClient::*_blocking`, `MtgjsonClient::*_blocking`) for GUI apps that call APIs from worker
  threads without an async runtime. `check_stock` enables this;
  `inventory_sync` uses the async variants.

## Async vs blocking

All client logic is written once; the blocking variants are thin
`reqwest::blocking` counterparts of the async functions. When adding a new
endpoint, implement the async version and add a blocking wrapper only if a
GUI project needs it.

## Development

```bash
./run_quality_checks.sh   # fmt, clippy, tests, build (all features)
```
