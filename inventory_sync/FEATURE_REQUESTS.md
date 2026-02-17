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
                                                        ▼ every 1h (configurable)
                                             ┌─────────────────────────┐
                                             │   Price Sources         │
                                             │   (Cardmarket CDN)      │
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
**Description**: Background job that fetches prices on a configurable schedule without intervention.
- Runs continuously as a server daemon
- Scheduled price fetching (default: every 1 hour, configurable via `--interval-hours`)
- Fetches from Cardmarket CDN price guide
- Store historical price data with timestamps
- Berlin timezone handling for correct date attribution (Cardmarket timestamps are CET/CEST)

**Status**: Fully Implemented (2026-02-17)
- ✅ Fetches Cardmarket price guide on startup (implemented 2026-02-01)
- ✅ Historical price storage - one entry per product per day (implemented 2026-02-01)
- ✅ Daemon mode with configurable check intervals (implemented 2026-02-14)
- ✅ Daily deduplication prevents duplicate price entries (implemented 2026-02-01)
- ✅ Berlin timezone-aware date handling for server-agnostic operation (implemented 2026-02-17)

### 3. Server Runtime
**Description**: Long-running server process in Docker.
- HTTP server (axum or actix-web)
- Background task scheduler for price collection
- Graceful shutdown handling (responds to SIGTERM)
- Configurable via environment variables
- Dockerfile for building the container image
- docker-compose.yml for local development

**Status**: Fully Implemented (2026-02-14)
- ✅ Axum HTTP server (implemented 2026-02-14)
- ✅ Background daemon mode with configurable intervals (implemented 2026-02-14)
- ✅ Atomic database writes for safe shutdown (implemented 2026-02-01)
- ✅ CLI configuration via clap (implemented 2026-02-01)
- ✅ Dockerfile with multi-stage build (implemented 2026-02-01)
- ✅ docker-compose.yml (implemented 2026-02-01)

### 4. Web UI for Price Tracking
**Description**: Modern web interface for browsing card data and viewing price history.
- Real-time card search with fuzzy matching
- Interactive price charts showing trend/avg/low prices
- Card image display from Scryfall API
- Server-side image caching for performance
- Mobile-responsive design

**Status**: Fully Implemented (2026-02-17)
- ✅ Modern dark-themed UI with responsive design (implemented 2026-02-14)
- ✅ Real-time search API with debouncing (implemented 2026-02-14)
- ✅ Chart.js integration for price visualization (implemented 2026-02-14)
- ✅ Scryfall API integration for card images and metadata (implemented 2026-02-14)
- ✅ Persistent server-side image and metadata cache by product ID (implemented 2026-02-14)
- ✅ 24-hour browser cache headers (implemented 2026-02-14)
- ✅ Technical indicators: EMA, SMA, Bollinger Bands, RSI, MACD (implemented 2026-02-17)
- ✅ Card metadata display: set name, type, mana cost, rarity, oracle text (implemented 2026-02-17)
- ✅ Cardmarket purchase link on card titles (implemented 2026-02-17)

---

## How to Request Features

Add new feature requests to the appropriate priority section with:
1. A clear title
2. Brief description of the feature
3. Use cases and expected behavior
