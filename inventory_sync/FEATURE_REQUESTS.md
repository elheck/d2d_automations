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

### 3. Server Runtime
**Description**: Long-running server process in Docker.
- HTTP server (axum or actix-web)
- Background task scheduler for price collection
- Graceful shutdown handling (responds to SIGTERM)
- Configurable via environment variables
- Dockerfile for building the container image
- docker-compose.yml for local development

**Status**: Planned

---

## Code Review Findings & Critical Gaps

*Last reviewed: 2026-02-12*

### Overall Assessment: Grade C+ (65/100)

**Status**: ⚠️ **NOT PRODUCTION READY** - Core server functionality missing

**Current State**: Functional batch price collector (30-40% complete), **NOT** the planned REST API server. Database layer is excellent, but entire API/server layer is missing.

### 🚨 CRITICAL - Blocking Issues for Server Deployment

#### CRITICAL-1: REST API Framework Not Implemented (0%)
**Issue**: No web server framework integrated
- **Missing**: No axum/actix-web in dependencies
- **Missing**: No HTTP endpoints (POST /sync, GET /cards, GET /prices)
- **Missing**: No routing, handlers, request/response types
- **Required by**: Feature Request #1 (lines 50-55)
- **Effort**: 3-5 days
- **Priority**: BLOCKING

#### CRITICAL-2: API Authentication Missing (0%)
**Issue**: Would be completely open if server were added
- **Missing**: No API key validation
- **Missing**: No bearer token support
- **Missing**: No authentication middleware
- **Required by**: Security section (line 41)
- **Security Risk**: CRITICAL - Unauthenticated access
- **Effort**: 1-2 days
- **Priority**: BLOCKING

#### CRITICAL-3: Localhost Binding Not Implemented (0%)
**Issue**: No network security controls
- **Missing**: No default 127.0.0.1 binding
- **Missing**: No external exposure configuration
- **Required by**: Security section (line 43)
- **Security Risk**: CRITICAL - Could expose to network
- **Effort**: 1 day
- **Priority**: BLOCKING

#### CRITICAL-4: Rate Limiting Missing (0%)
**Issue**: Vulnerable to denial of service attacks
- **Missing**: No rate limiting middleware
- **Missing**: No per-client tracking
- **Required by**: Security section (line 44)
- **Security Risk**: CRITICAL - DoS vulnerability
- **Effort**: 1-2 days
- **Priority**: BLOCKING

#### CRITICAL-5: API Error Sanitization Missing (0%)
**Issue**: Would expose internal errors to clients
- **Current**: Error enum includes full error details ([src/error.rs](src/error.rs))
- **Missing**: Separation of internal vs API errors
- **Required by**: Security section (line 46)
- **Security Risk**: HIGH - Stack trace exposure
- **Effort**: 1-2 days
- **Priority**: BLOCKING

### HIGH Priority - Core Missing Features

#### 6. CSV Import Functionality (0%)
**Issue**: CSV parsing not implemented despite being in project description
- **Status**: Feature Request #1 (line 52) says "Accept CSV data"
- **Current**: No CSV import code exists
- **Effort**: 2-3 days
- **Priority**: HIGH

#### 7. Database Error Integration
**Issue**: Database errors not integrated into Error enum
- **Location**: [src/error.rs](src/error.rs)
- **Missing**: Database(rusqlite::Error) variant
- **Effort**: 1 day
- **Priority**: HIGH

#### 8. Integration Tests (0%)
**Issue**: Zero integration tests
- **Missing**: No tests/ directory
- **Missing**: No test fixtures
- **Required by**: CLAUDE.md testing patterns
- **Effort**: 2-3 days
- **Priority**: HIGH

### MEDIUM Priority - Improvements

#### 9. Background Job Scheduling
**Issue**: Hourly checks with daily dedup, not true 12-hour schedule
- **Current**: Checks every hour, skips if data exists for today
- **Needed**: Cron-like scheduler at 3am and 3pm daily
- **Location**: [src/main.rs:80-92](src/main.rs#L80-L92)
- **Effort**: 2 days
- **Priority**: MEDIUM

#### 10. Health Endpoint
**Issue**: No health check for orchestration
- **Required by**: Requirements section (line 34)
- **Effort**: 0.5 day
- **Priority**: MEDIUM

#### 11. Graceful Shutdown
**Issue**: No SIGTERM handler for server
- **Note**: Already safe due to transactional DB writes
- **Needed**: Clean server shutdown for in-flight requests
- **Effort**: 1 day
- **Priority**: MEDIUM

### Security Audit: ⚠️ MIXED

| Aspect | Status | Notes |
|--------|--------|-------|
| SQL Injection Prevention | ✅ EXCELLENT | Perfect parameterized queries |
| Atomic DB Writes | ✅ EXCELLENT | All transactional, safe shutdown |
| No Hardcoded Secrets | ✅ GOOD | Ready for env vars |
| Docker Security | ✅ GOOD | Non-root user, minimal image |
| API Authentication | ❌ MISSING | Critical gap |
| Localhost Binding | ❌ MISSING | Critical gap |
| Rate Limiting | ❌ MISSING | Critical gap |
| Error Sanitization | ❌ MISSING | Would expose internals |

### What's Working Well ✅

- **Database Layer**: Excellent schema, proper indexes, foreign keys
- **SQL Safety**: Perfect parameterized queries ([src/database.rs](src/database.rs))
- **Transactions**: All writes atomic, safe to kill at any time
- **Cardmarket Client**: Complete implementation with tests
- **Docker Setup**: Multi-stage build, proper volumes

### Implementation Roadmap

**Phase 1: Security Foundation (1-2 weeks)**
1. Add API authentication middleware
2. Implement localhost binding + config
3. Add rate limiting
4. Create API error sanitization

**Phase 2: Core REST API (2-3 weeks)**
5. Integrate axum framework
6. Implement GET /cards endpoint
7. Implement GET /prices/{card_id} endpoint
8. Add CSV import functionality
9. Implement POST /sync endpoint

**Phase 3: Production Readiness (1-2 weeks)**
10. Add integration tests + fixtures
11. Implement graceful shutdown
12. Add health endpoint
13. Proper 12-hour scheduled jobs

**Estimated Time to Production**: 4-6 weeks

### Decision Required

**Option A**: Rename project to "cardmarket_price_collector" to match actual functionality
**Option B**: Complete REST API implementation (4-6 weeks effort)

**Recommendation**: Do not deploy as "REST API server" until Phase 1 (security) is 100% complete.

---

## How to Request Features

Add new feature requests to the appropriate priority section with:
1. A clear title
2. Brief description of the feature
3. Use cases and expected behavior
