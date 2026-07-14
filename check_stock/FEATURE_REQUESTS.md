# Feature Requests

This document contains feature requests for the Check Stock Application.


## High Priority

### 1. Repricing action export from the Mispricing Report

The report finds under/over-priced cards but stays read-only, so acting on it
means manual work per card. Add configurable repricing rules (e.g. "set to
trend − 5 %, never below X, cap change at Y %"), a review checklist like the
consolidation move list, and export of the ticked rows as a Cardmarket
stock-update CSV — same pattern the Discard flow already uses.

**Decision it enables:** capture the repricing upside the report already
quantifies, in minutes instead of hours, while keeping a human in the loop.

## Medium Priority

### 2. Price-guide history & price-movement alerts

Fetching the price guide gives a point-in-time view only. Persist a daily (or
per-fetch) trend price per in-stock product into SQLite and surface movers:
cards whose market trend rose/fell more than N % over 7/30 days, with your
listed price alongside.

**Decision it enables:** catch spikes early (sell into them / raise price
before the market corrects) and spot falling knives worth liquidating before
the trend erodes further.

### 3. Dead-stock liquidation planner

Combine the aging buckets with the mispricing data: for stock older than a
chosen threshold, propose a markdown schedule (e.g. −10 % at 90 days, −25 % at
180, bulk-out below €0.30) and show the capital freed vs. revenue given up at
each step. Export as a stock-update CSV like feature 1.

**Decision it enables:** a concrete, costed plan to convert the "capital tied
up" number the aging report already shows into cash and free bin space.

### 4. Buy Helper × own-stock/demand cross-check

When valuing an offer, flag each card with what you already know: current
copies in stock, its sales velocity (from the sold-events log), and dead-stock risk
(you hold 12 copies that haven't moved in 6 months). Optionally auto-price
singles from the price-guide trend instead of the seller's CSV prices.

**Decision it enables:** avoid paying singles rates for cards that will rot in
a bin, and bid confidently on cards your own history proves sell fast.

### 5. Inventory turnover & cash-flow dashboard

From the snapshot history: chart in-stock value and revenue/week over time,
plus derived KPIs — inventory turnover (annualized revenue ÷ average stock
value), weeks-of-stock at current velocity, and average days-to-sale.

**Decision it enables:** answers "is the business speeding up or slowing
down?" and "is capital growing faster than sales?" at a glance — the single
most useful health check for a reselling operation.

## Low Priority

### 6. Concentration & exposure report

Break current stock value down by set and show concentration risk: % of total
value in the top 10 cards / top 3 sets, and value held in cards above a price
threshold (reprint-sensitive capital).

**Decision it enables:** whether a reprint announcement or format shift could
wipe out a meaningful share of inventory value, and when to diversify or sell
down a position.

### 7. Average sale price vs. current listing price

Using the sold-events log: for variants still in stock, compare the average
realized sale price against the current listed price and the market trend.

**Decision it enables:** spot listings priced below what buyers demonstrably
paid before (free margin) and listings priced above what they ever sold at.

### 8. Wantslist demand mining

Count how often cards appear across checked wantslists/decklists over time and
match against stock.

**Decision it enables:** a local demand signal for restocking and pricing —
cards repeatedly requested but rarely in stock are safe buys.



### Strengths to Maintain

- Excellent test fixtures (reused across tests)
- Clean error handling (custom ApiError enum)
- Comprehensive performance tests
- Zero clippy warnings
