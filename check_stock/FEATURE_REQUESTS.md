# Feature Requests

This document contains feature requests for the Check Stock Application.

## Business Analytics & Pricing Roadmap

*Added: 2026-07-14. Derived from a capability review of the price guide + inventory
DB assets. The **Mispricing / Margin Report** (its own screen) and **Sales Velocity &
Dead-Stock Aging** (added to the Stock Analysis screen) from this batch are ✅ **done**;
everything below is queued for later.*

### Tier 1 — Highest business impact

#### Automated repricing against the market → Cardmarket-uploadable CSV
The highest-leverage feature. The Cardmarket price guide (`trend/avg/low/avg1/avg7/avg30`,
foil & non-foil) is already fetched and wired into the pricing nodes via
`CachedLatestPrice`. Turn the read-only Mispricing report into an *action*:
- Let the Pricing node graph **emit** a repriced CSV, not just filter. This is the
  intended use for the deliberately-removed price-transform nodes (`PriceMultiply`,
  `PriceFloor`, …) — re-add them here. Rules like "trend × 0.95, floor 0.10, round to
  Cardmarket price ticks", configurable per condition/rarity.
- Export in Cardmarket's stock-update format so the loop closes:
  export → reprice → re-upload.
- **Reuses**: `PriceGuide`, `CachedLatestPrice::price_for`, existing node eval, and the
  new `mispricing` module.
- **Security note**: this feature *writes prices meant for upload*. Keep a mandatory
  dry-run/preview + explicit confirm before any export; never auto-upload.
- **Effort**: medium-high (mostly reconnecting existing pieces).

### Tier 2 — Strong quality-of-life

#### Richer wantslist / decklist import ✅ Done
Implemented in the pure [`wantslist`](src/wantslist.rs) module (delegated to from
`io::read_wantslist`). Parses plain `quantity name`, `4x`/`4X` quantities, and MTG
Arena / MTGO / Moxfield / Archidekt / MTGGoldfish exports: strips set codes,
collector numbers, foil/etched markers, `[category]` / `^tag^` annotations, `SB:`
prefixes and section headers, and merges duplicate names. Deck-based `.dek` XML was
intentionally left out (all target tools export the supported text format).

Additionally, the Wantslist field accepts a pasted **Moxfield / Archidekt deck link**
([`deck_fetch`](src/deck_fetch.rs)): the deck is fetched from each site's public JSON
API and converted to wants (`io::load_wantslist` dispatches URL vs file). JSON shaping
is pure/tested; only the HTTP call needs the network.

#### Picking route optimization
`parse_location_code` already decodes `A-S-R-C`. Sort the picking list into an actual
**walk order** (aisle serpentine) instead of card order — real time savings on
multi-item orders.
- **Location**: `ui/screens/picking.rs`, `card_matching::parse_location_code`.
- **Effort**: low-medium.

#### Bin consolidation suggestions ✅ Done
Implemented in the pure [`bin_consolidation`](src/bin_consolidation.rs) module, surfaced
in the Bin Analysis screen. Empties bins filled at/below a chosen threshold (slider up to
the full 60-card bin) by spreading each bin's piles across other bins that have room —
only when a whole bin can be cleared, and never moving a card twice. Each pile's target
is chosen by a multi-factor ranking: prefer a **keeper** bin (not itself scheduled to be
emptied) → a bin **already holding the same variant** (de-fragmentation) → the **closest**
bin by weighted aisle/shelf/row/column proximity → tightest pack → name. Reports the
total move distance and exports a Cardmarket-style move CSV via `to_update_csv`. An
**interactive move list** ([`ui/screens/consolidation.rs`](src/ui/screens/consolidation.rs))
renders the moves as Scryfall card-image tiles grouped by source bin, each with a
destination and a "Mark Moved" toggle + progress bar (mirrors the picking list).
**Read-only**: never writes to the inventory DB (moves apply on the next CSV re-load);
each pile keeps its lot/side suffix, so per-lot revenue tracking is undisturbed.

A dedicated **fragmented-variant report** (`fragmented_variants`) plus a threshold-independent
**defrag plan** (`plan_variant_defrag`) that gathers each scattered variant into one bin are
implemented and surfaced in the Bin Analysis screen (own section, report + move list + CSV).
The sparse-bin planner is no longer a single greedy pass: `plan_consolidation` runs several
source-ordering strategies and keeps the best plan (most bins freed, then least distance, then
fewest cards) — a stronger heuristic, still not a proven global optimum. The interactive move
list can be opened for **all** piles or for a **single chosen source bin**.

Follow-up ideas: tunable factor weights in the UI; a true globally-optimal repack (ILP/branch
& bound) if the greedy ensemble proves insufficient; an in-app "apply to DB" that rewrites
locations without a CSV round-trip (would need care around the one-location-per-variant DB model).

#### Buy Helper → market-aware valuation
Buy Helper values singles off the CSV's own `price` column (the seller's export).
Optionally value against the **live price guide** instead for accuracy. Stays
strictly read-only.
- **Location**: `buy_helper.rs` (`compute_summary`/`classify`), `api::cardmarket::PriceGuide`.
- **Effort**: low.

### Tier 3 — Robustness / polish

#### Price-guide freshness indicator
Show when the ~50MB price guide was last fetched and auto-refresh if stale. Cheap trust
signal for any repricing decision.

#### CSV import validation report
Surface how many rows loaded, skipped, or had unparseable prices/conditions on import,
instead of silently defaulting to `0`.
- **Location**: `io::read_csv`, screens that call it.

#### Persist Pricing node graphs
Save/load named node graphs so repricing recipes are reusable across sessions.
- **Location**: `ui/state.rs` (`NodeGraph`, already `Serialize`/`Deserialize`), pricing screen.





### High Priority Improvements


#### 1. Implement Rate Limiting for Scryfall API
**Issue**: Semaphore limits concurrency but not requests/second (Scryfall limit: 10 req/s)
- **Location**: [src/ui/screens/picking.rs:105](src/ui/screens/picking.rs#L105)
- **Fix**: Add proper rate limiter (e.g., `governor` crate)
- **Effort**: 1 day
- **Priority**: MEDIUM

### Medium Priority Improvements

### Low Priority

#### 4. Add Module Documentation
- 34 doc comments exist, but 63 public functions
- Missing detailed docs in io.rs, formatters.rs
- Add rustdoc examples for public APIs

#### 5. Optimize String Allocations
- Profile first before optimizing
- Format strings in card matching hot paths
- Likely not a bottleneck with typical inventory sizes

### Security Audit: ✅ EXCELLENT

- **SQL Injection**: N/A (no SQL, CSV-based)
- **Input Validation**: Comprehensive CSV and wantslist parsing
- **No Secrets in Code**: ✅ Clean
- **No Unsafe Code**: ✅ Zero unsafe blocks
- **API Security**: User-Agent headers set, proper error handling


### Strengths to Maintain

- Excellent test fixtures (reused across tests)
- Clean error handling (custom ApiError enum)
- Comprehensive performance tests
- Zero clippy warnings
