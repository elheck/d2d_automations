# Feature Requests for Inventory Sync

This document tracks feature requests for the inventory_sync service.

## Overview

inventory_sync is a standalone server application that runs continuously on a separate server in a Docker container. It collects the Cardmarket price guide and product catalog on a schedule, stores them in SQLite, and serves them to `check_stock` and the web UI. Inventory itself is not synced here: `check_stock` keeps it in its own local database.


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

No open feature requests. (The planned `POST /sync` / `GET /cards` inventory
API was dropped: `check_stock` keeps inventory locally, and the server only
tracks prices.) The security requirements above are not all met yet — see the
review table in the root README.

## How to Request Features

Add new feature requests to the appropriate priority section with:
1. A clear title
2. Brief description of the feature
3. Use cases and expected behavior
