# Feature Requests for Inventory Sync

This document tracks feature requests for the inventory_sync service.

## Overview

inventory_sync is a standalone server application that runs continuously on a separate server. It provides an API for syncing card inventory to a SQLite database and automatically collects prices on a schedule.

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

**Status**: Planned

### 3. SQLite Database
**Description**: Persistent storage for cards and price history.
- Cards table with card metadata
- Price history table with timestamps and source
- Efficient queries for price trends

**Status**: Planned

### 4. Server Runtime
**Description**: Long-running server process.
- HTTP server (axum or actix-web)
- Background task scheduler for price collection
- Graceful shutdown handling
- Configurable via environment variables or config file

**Status**: Planned

## Completed

_No features completed yet_



## How to Request Features

Add new feature requests to the appropriate priority section with:
1. A clear title
2. Brief description of the feature
3. Use cases and expected behavior
