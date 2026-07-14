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

#### Richer wantslist / decklist import
Stock Checker currently only parses `quantity name`. Add importers for **Moxfield /
Archidekt / MTGO `.dek` / MTG Arena** export formats, plus set-scoped lines. Makes the
Stock Checker usable directly from how buylists and deck requests actually arrive.
- **Location**: `io::read_wantslist`, `card_matching`.
- **Effort**: medium.

#### Picking route optimization
`parse_location_code` already decodes `A-S-R-C`. Sort the picking list into an actual
**walk order** (aisle serpentine) instead of card order — real time savings on
multi-item orders.
- **Location**: `ui/screens/picking.rs`, `card_matching::parse_location_code`.
- **Effort**: low-medium.

#### Bin consolidation suggestions
Bin Analysis reports free slots; the inverse is more actionable: "these half-empty bins
for lot L4 could merge", and flag the same variant fragmented across many locations
(the DB already sums those).
- **Location**: `stock_analysis.rs` (bin logic), `inventory_db`.
- **Effort**: medium.

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
