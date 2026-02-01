# Feature Requests for Inventory Sync

This document tracks feature requests for the inventory_sync service.

## Overview

inventory_sync is a standalone server application that runs continuously on a separate server in a Docker container. It provides an API for syncing card inventory to a SQLite database and automatically collects prices on a schedule.

## Architecture

```
┌─────────────────┐         HTTP API         ┌─────────────────────────┐
│   check_stock   │ ──────────────────────▶  │    inventory_sync       │
│   (desktop)     │    POST /sync            │    (server)             │
└─────────────────┘                          │                         │
                                             │  - REST API             │
                                             │  - SQLite database      │
                                             │  - Scheduled price jobs │
                                             └─────────────────────────┘
                                                        │
                                                        ▼ every 12h
                                             ┌─────────────────────────┐
                                             │   Price Sources         │
                                             │   (Scryfall, etc.)      │
                                             └─────────────────────────┘
```

## Requirements

### Deployment
- **Docker-based**: The application must run in a Docker container for consistent deployment and isolation.
- **Volume mounts**: SQLite database file must be stored on a mounted volume for persistence across container restarts.
- **Environment configuration**: All configuration (ports, API keys, database path) via environment variables.
- **Health checks**: Container should expose a health endpoint for orchestration tools.

### Data Integrity
- **Safe shutdown**: The server must be safe to quit at any time without risking data integrity. All database writes must be atomic and transactional.
- **No unsanitized SQL**: Under no circumstances may unsanitized SQL be used. All queries must use parameterized statements / prepared statements to prevent SQL injection.

### Security
- **API Authentication**: All endpoints require authentication via API key or bearer token. No anonymous access.
- **Input Validation**: Validate and sanitize all incoming data (CSV content, card fields). Reject malformed or suspicious input. Limit payload sizes to prevent memory exhaustion.
- **Network Security**: Bind to `127.0.0.1` (localhost) by default. Require explicit configuration to expose externally. Use HTTPS/TLS if exposed to network.
- **Rate Limiting**: Protect against abuse and denial of service attacks with request rate limits per client.
- **Secure Configuration**: All secrets (API keys, tokens) must come from environment variables. Never log sensitive data. Never hardcode credentials.
- **Error Handling**: Never expose internal error details or stack traces to API clients. Log details server-side only.

## Features

### 1. REST API for Card Sync
**Description**: Expose an HTTP API that check_stock can call to sync cards.
- `POST /sync` - Accept CSV data or card list, store in SQLite
- `GET /cards` - Query stored cards
- `GET /prices/{card_id}` - Get price history for a card
- Upsert logic (insert new, update existing)

**Status**: Planned

### 2. Automated Price Collection
**Description**: Background job that fetches prices every 12 hours without intervention.
- Runs continuously as a server daemon
- Scheduled price fetching (every 12 hours)
- Support multiple price sources (Scryfall, Cardmarket, etc.)
- Store historical price data with timestamps
- Graceful handling of API rate limits

**Status**: Partially Implemented
- ✅ Fetches Cardmarket price guide on startup (implemented 2026-02-01)
- ✅ Historical price storage - one entry per product per day (implemented 2026-02-01)
- ⏳ Scheduled background job (pending - currently manual trigger only)

### 3. SQLite Database
**Description**: Persistent storage for cards and price history.
- Cards table with card metadata
- Price history table with timestamps and source
- Efficient queries for price trends

**Status**: ✅ Implemented (2026-02-01)
- `products` table: 120k+ MTG products with name, category, expansion, metacard
- `price_history` table: Daily price snapshots with composite key (id_product, price_date)
- CLI flag `--database` for custom database path
- Default path: `~/.local/share/inventory_sync/inventory.db`

### 4. Server Runtime
**Description**: Long-running server process in Docker.
- HTTP server (axum or actix-web)
- Background task scheduler for price collection
- Graceful shutdown handling (responds to SIGTERM)
- Configurable via environment variables
- Dockerfile for building the container image
- docker-compose.yml for local development

**Status**: Planned

## Completed

### Product Catalog Fetching (2026-02-01)
- Fetches Cardmarket product catalog (singles + non-singles, ~120k products)
- Correlates product IDs with names for price history queries

### Historical Price Collection (2026-02-01)
- Fetches Cardmarket price guide with trend, avg, low prices
- Stores daily snapshots in SQLite with deduplication
- Preserves historical data (never overwrites existing entries for same date)
- Parameterized queries for SQL injection prevention
- Transactional writes for data integrity



## How to Request Features

Add new feature requests to the appropriate priority section with:
1. A clear title
2. Brief description of the feature
3. Use cases and expected behavior
