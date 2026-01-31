# Feature Requests for Inventory Sync

This document tracks feature requests and enhancement ideas for the inventory_sync application.

## High Priority

### 1. CSV Import from Cardmarket
**Description**: Parse Cardmarket CSV exports and store card data in SQLite
- Parse semicolon-separated CSV format
- Handle multi-item fields with ` | ` delimiter
- Store card metadata (name, set, condition, language, etc.)
- Support incremental updates (upsert pattern)

**Priority**: High
**Status**: Planned

### 2. SQLite Database Layer
**Description**: Persistent storage with efficient querying
- Cards table with Cardmarket IDs as primary key
- Price history table for tracking changes over time
- Full-text search indexes for card names
- Schema migrations for future updates

**Priority**: High
**Status**: Planned

### 3. Scryfall Price Integration
**Description**: Fetch current prices from Scryfall API
- Match cards by set code and collector number
- Store both regular and foil prices
- Respect API rate limits (50ms between requests)
- Record price history with timestamps

**Priority**: High
**Status**: Planned

## Medium Priority

### 4. CLI Interface
**Description**: Command-line interface for all operations
- `sync` command for CSV import
- `prices` command for price collection
- `stats` command for database statistics
- `daemon` command for scheduled operation

**Priority**: Medium
**Status**: Planned

### 5. Daemon Mode with Scheduling
**Description**: Background service for automated sync and price collection
- Configurable sync intervals
- Cron-style scheduling support
- Graceful shutdown handling

**Priority**: Medium
**Status**: Planned

## Low Priority

### 6. Price Alerts
**Description**: Notify when prices cross thresholds
- User-defined price thresholds per card
- Support for both price increases and decreases

**Priority**: Low
**Status**: Future consideration

### 7. Multiple Price Sources
**Description**: Aggregate prices from multiple sources
- Cardmarket price guide integration
- TCGPlayer API support

**Priority**: Low
**Status**: Future consideration

## Completed

_No features completed yet_

---

## How to Request Features

Add new feature requests to the appropriate priority section with:
1. A clear title
2. Brief description of the feature
3. Use cases and expected behavior
