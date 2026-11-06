# Feature Requests

This document contains feature requests for the Check Stock Application.


## High Priority

_Nothing open._

## Medium Priority

### 1. Dead-stock liquidation planner

Markdown rules themselves now exist in the Repricing screen ("no sale for 180
days → current price −10%", aging conditions, export). Still missing: a
costed view of a whole schedule (e.g. −10 % at 90 days, −25 % at 180) that
shows the capital freed vs. revenue given up at each step, and a hand-off of
the leftovers to the Bulk Lots screen.

**Decision it enables:** a concrete, costed plan to convert the "capital tied
up" number the aging report already shows into cash and free bin space.

### 2. Buy Helper × own-stock/demand cross-check

When valuing an offer, flag each card with what you already know: current
copies in stock, its sales velocity (from the sold-events log), and dead-stock risk
(you hold 12 copies that haven't moved in 6 months). Optionally auto-price
singles from the price-guide trend instead of the seller's CSV prices.

**Decision it enables:** avoid paying singles rates for cards that will rot in
a bin, and bid confidently on cards your own history proves sell fast.

### 3. Inventory turnover & cash-flow dashboard

From the snapshot history: chart in-stock value and revenue/week over time,
plus derived KPIs — inventory turnover (annualized revenue ÷ average stock
value), weeks-of-stock at current velocity, and average days-to-sale.

**Decision it enables:** answers "is the business speeding up or slowing
down?" and "is capital growing faster than sales?" at a glance — the single
most useful health check for a reselling operation.

## Low Priority

### 4. Concentration & exposure report

Break current stock value down by set and show concentration risk: % of total
value in the top 10 cards / top 3 sets, and value held in cards above a price
threshold (reprint-sensitive capital).

**Decision it enables:** whether a reprint announcement or format shift could
wipe out a meaningful share of inventory value, and when to diversify or sell
down a position.

### 5. Average sale price vs. current listing price

Using the sold-events log: for variants still in stock, compare the average
realized sale price against the current listed price and the market trend.

**Decision it enables:** spot listings priced below what buyers demonstrably
paid before (free margin) and listings priced above what they ever sold at.

### 6. Wantslist demand mining

Count how often cards appear across checked wantslists/decklists over time and
match against stock.

**Decision it enables:** a local demand signal for restocking and pricing —
cards repeatedly requested but rarely in stock are safe buys.



### Strengths to Maintain

- Excellent test fixtures (reused across tests)
- Clean error handling (custom ApiError enum)
- Comprehensive performance tests
- Zero clippy warnings
