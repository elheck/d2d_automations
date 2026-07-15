# mtg_common

Shared library for the MTG business projects in this repo (`check_stock`,
`inventory_sync`). Holds everything that used to be copy-pasted between
projects: API types, HTTP clients, and caching primitives.

## Contents

| Module | Contents |
|--------|----------|
| `scryfall` | `ScryfallCard` (superset of fields used across projects), `ImageUris`, `CardFace`, `ScryfallPrices`, `PurchaseUris`, and fetch functions (`fetch_card`, `fetch_card_by_cardmarket_id`, `fetch_card_by_name`, `fetch_image`) |
| `cardmarket` | `PriceGuide` (lookup by product ID with `load`/`fetch`), `PriceGuideEntry`, `PriceGuideFile` |
| `file_cache` | `FileCache` — best-effort persistent byte cache backed by files in a directory; foundation for the projects' image caches |
| `error` | `MtgError` / `MtgResult` — common error type; projects convert it into their own error types via `From` |

Also exports `USER_AGENT` (shared User-Agent header for all external API
requests) and `PRICE_GUIDE_URL`.

## Features

- `blocking` — enables blocking (non-async) variants of the HTTP clients
  (`scryfall::blocking::*`, `PriceGuide::fetch_blocking`) for GUI apps that
  call APIs from worker threads without an async runtime. `check_stock`
  enables this; `inventory_sync` uses the async variants.

## Async vs blocking

All client logic is written once; the blocking variants are thin
`reqwest::blocking` counterparts of the async functions. When adding a new
endpoint, implement the async version and add a blocking wrapper only if a
GUI project needs it.

## Development

```bash
./run_quality_checks.sh   # fmt, clippy, tests, build (all features)
```
