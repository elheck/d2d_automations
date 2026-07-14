# Feature Requests

This document contains feature requests for the Check Stock Application.

The features below focus on turning existing data (per-variant `sold_quantity`,
daily snapshots, the Cardmarket price guide, lot locations, `listed_at` dates)
into better business decisions: what to buy, what to reprice, what to write off,
and which sourcing channels actually make money.

## High Priority

### 1. Per-card sales history (sold-events table)

The DB already tracks cumulative `sold_quantity` per variant, but snapshots are
aggregate-only, so "what sold" is invisible below the whole-inventory level.
On each sync, record per-variant sold deltas into a `sold_events` table
(`date, variant key, copies, listed price at sale`).

**Decision it enables:** the foundation for everything below — top sellers,
restock lists, average realized price vs. current listing price, and velocity
broken down by set / rarity / price band instead of one global number.

### 2. Restock recommendations ("sold out, sold fast")

List variants at quantity 0 with `sold_quantity > 0`, ranked by how quickly
they sold (listed → sold-out span, copies/week) and realized revenue. Filter
out one-off sales. Export as a buy list.

**Decision it enables:** what to actively source at buylist/trade — currently
the app only says what *isn't* selling (aging report), not what to buy again.

### 3. Acquisition cost & margin tracking per lot

The lot breakdown tracks revenue but not cost, so profitability is unknown.
Let the Buy Helper (or a manual entry) record a purchase's total cost against a
lot ID. Stock Analysis then shows per-lot: cost basis, revenue to date,
remaining stock value, realized margin %, and payback status (recouped or not).

**Decision it enables:** which sourcing channels/deals are actually profitable,
what discount to demand on the next collection buy, and when a lot has paid for
itself so the remainder can be marked down aggressively.

### 4. Repricing action export from the Mispricing Report

The report finds under/over-priced cards but stays read-only, so acting on it
means manual work per card. Add configurable repricing rules (e.g. "set to
trend − 5 %, never below X, cap change at Y %"), a review checklist like the
consolidation move list, and export of the ticked rows as a Cardmarket
stock-update CSV — same pattern the Discard flow already uses.

**Decision it enables:** capture the repricing upside the report already
quantifies, in minutes instead of hours, while keeping a human in the loop.

## Medium Priority

### 5. Price-guide history & price-movement alerts

Fetching the price guide gives a point-in-time view only. Persist a daily (or
per-fetch) trend price per in-stock product into SQLite and surface movers:
cards whose market trend rose/fell more than N % over 7/30 days, with your
listed price alongside.

**Decision it enables:** catch spikes early (sell into them / raise price
before the market corrects) and spot falling knives worth liquidating before
the trend erodes further.

### 6. Dead-stock liquidation planner

Combine the aging buckets with the mispricing data: for stock older than a
chosen threshold, propose a markdown schedule (e.g. −10 % at 90 days, −25 % at
180, bulk-out below €0.30) and show the capital freed vs. revenue given up at
each step. Export as a stock-update CSV like feature 4.

**Decision it enables:** a concrete, costed plan to convert the "capital tied
up" number the aging report already shows into cash and free bin space.

### 7. Buy Helper × own-stock/demand cross-check

When valuing an offer, flag each card with what you already know: current
copies in stock, its sales velocity (from feature 1), and dead-stock risk
(you hold 12 copies that haven't moved in 6 months). Optionally auto-price
singles from the price-guide trend instead of the seller's CSV prices.

**Decision it enables:** avoid paying singles rates for cards that will rot in
a bin, and bid confidently on cards your own history proves sell fast.

### 8. Inventory turnover & cash-flow dashboard

From the snapshot history: chart in-stock value and revenue/week over time,
plus derived KPIs — inventory turnover (annualized revenue ÷ average stock
value), weeks-of-stock at current velocity, and average days-to-sale.

**Decision it enables:** answers "is the business speeding up or slowing
down?" and "is capital growing faster than sales?" at a glance — the single
most useful health check for a reselling operation.

## Low Priority

### 9. Concentration & exposure report

Break current stock value down by set and show concentration risk: % of total
value in the top 10 cards / top 3 sets, and value held in cards above a price
threshold (reprint-sensitive capital).

**Decision it enables:** whether a reprint announcement or format shift could
wipe out a meaningful share of inventory value, and when to diversify or sell
down a position.

### 10. Average sale price vs. current listing price

Using sold events (feature 1): for variants still in stock, compare the average
realized sale price against the current listed price and the market trend.

**Decision it enables:** spot listings priced below what buyers demonstrably
paid before (free margin) and listings priced above what they ever sold at.

### 11. Wantslist demand mining

Count how often cards appear across checked wantslists/decklists over time and
match against stock.

**Decision it enables:** a local demand signal for restocking and pricing —
cards repeatedly requested but rarely in stock are safe buys.

### Strengths to Maintain

- Excellent test fixtures (reused across tests)
- Clean error handling (custom ApiError enum)
- Comprehensive performance tests
- Zero clippy warnings
