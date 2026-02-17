# Feature Requests

This document contains feature requests for the Check Stock Application.

## High Priority

### MTG Price Monitor & Historical Database

**Goal**: Continuously monitor MTG card prices (from a watchlist or full inventory) and build a local database of historical price data for trend analysis.

#### Use Cases
- Track price trends over time to identify buy/sell opportunities
- Detect price spikes or drops on specific cards
- Build historical data for portfolio valuation
- Generate price alerts when thresholds are crossed

#### Proposed Features
- **Watchlist support**: Monitor specific cards or entire inventory
- **Scheduled polling**: Configurable intervals (hourly, daily) respecting API rate limits
- **Price sources**: Cardmarket (primary), optionally Scryfall/TCGPlayer
- **Local SQLite database**: Store historical snapshots with timestamps
- **Trend visualization**: Charts showing price history (30d, 90d, 1y)
- **Alerts**: Optional notifications when prices cross user-defined thresholds

#### Architecture Options
1. **Integrated into check_stock**: Add a "Price Monitor" screen with background polling
2. **Standalone application**: Separate `price_monitor/` project

#### Technical Considerations
- Cardmarket API rate limits (or scraping the price guide JSON)
- Delta storage vs full snapshots for database efficiency
- Background task scheduling (tokio cron or system scheduler)
- Could integrate with the planned SQLite + Litestream backup infrastructure

---


### Database Integration with Off-Site Backup

**Goal**: Store cards added via Card Lookup in a local SQLite database with automatic off-site backup to Cloudflare R2.

#### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Local Machine (On-Prem)                  │
│  ┌─────────────┐    ┌──────────────┐    ┌────────────────┐  │
│  │ Rust App    │───▶│ SQLite DB    │───▶│ Litestream     │  │
│  │ (check_stock)│    │ (cards.db)   │    │ (background)   │  │
│  └─────────────┘    └──────────────┘    └───────┬────────┘  │
└─────────────────────────────────────────────────┼───────────┘
                                                  │
                                                  ▼ Continuous real-time sync
                                    ┌─────────────────────────┐
                                    │  Cloudflare R2 (Free)    │
                                    │  - 10 GB storage         │
                                    │  - No egress fees        │
                                    │  - S3-compatible API     │
                                    └─────────────────────────┘
```

#### Why SQLite + Cloudflare R2 + Litestream?

- **SQLite**: Single-file database, zero config, handles millions of cards easily
- **Cloudflare R2**: 10GB free tier, no egress fees, S3-compatible
- **Litestream**: Real-time streaming replication, point-in-time recovery, designed for SQLite

#### Setup Requirements

##### 1. Cloudflare R2 Setup
1. Create Cloudflare account (free)
2. Enable R2 in dashboard
3. Create a bucket (e.g., `mtg-card-backup`)
4. Generate R2 API token with read/write permissions
5. Note the Account ID, Access Key ID, and Secret Access Key

##### 2. Litestream Installation
```bash
# Linux (Debian/Ubuntu)
wget https://github.com/benbjohnson/litestream/releases/download/v0.3.13/litestream-v0.3.13-linux-amd64.deb
sudo dpkg -i litestream-v0.3.13-linux-amd64.deb

# Linux (other)
wget https://github.com/benbjohnson/litestream/releases/download/v0.3.13/litestream-v0.3.13-linux-amd64.tar.gz
tar -xzf litestream-v0.3.13-linux-amd64.tar.gz
sudo mv litestream /usr/local/bin/

# macOS
brew install litestream

# Verify
litestream version
```

##### 3. Litestream Configuration
Create `/etc/litestream.yml`:
```yaml
dbs:
  - path: /home/user/.local/share/d2d_automations/cards.db
    replicas:
      - type: s3
        bucket: mtg-card-backup
        path: cards
        endpoint: https://<ACCOUNT_ID>.r2.cloudflarestorage.com
        access-key-id: <R2_ACCESS_KEY_ID>
        secret-access-key: <R2_SECRET_ACCESS_KEY>
```

Or use environment variables:
```bash
export LITESTREAM_ACCESS_KEY_ID=<R2_ACCESS_KEY_ID>
export LITESTREAM_SECRET_ACCESS_KEY=<R2_SECRET_ACCESS_KEY>
```

##### 4. Run Litestream

**Option A: Run manually (foreground)**
```bash
litestream replicate -config /etc/litestream.yml
```

**Option B: Run as systemd service (recommended)**
```bash
# Create service file
sudo tee /etc/systemd/system/litestream.service << EOF
[Unit]
Description=Litestream SQLite Replication
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/litestream replicate -config /etc/litestream.yml
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

# Enable and start
sudo systemctl daemon-reload
sudo systemctl enable litestream
sudo systemctl start litestream

# Check status
sudo systemctl status litestream
```

##### 5. Restore from Backup
```bash
# Restore to a new file (app should be stopped)
litestream restore -config /etc/litestream.yml \
  -o /home/user/.local/share/d2d_automations/cards.db \
  /home/user/.local/share/d2d_automations/cards.db

# Or restore to specific point in time
litestream restore -config /etc/litestream.yml \
  -o /tmp/cards-restored.db \
  -timestamp "2025-12-14T10:00:00Z" \
  /home/user/.local/share/d2d_automations/cards.db
```

#### Rust Implementation Tasks

1. **Add SQLite dependency**: `rusqlite` with bundled SQLite
2. **Create database schema**:
   ```sql
   CREATE TABLE cards (
       id INTEGER PRIMARY KEY,
       cardmarket_id TEXT,
       scryfall_id TEXT,
       name TEXT NOT NULL,
       set_code TEXT,
       collector_number TEXT,
       language TEXT,
       condition TEXT,
       is_foil BOOLEAN,
       quantity INTEGER DEFAULT 1,
       price REAL,
       location TEXT,
       added_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
       UNIQUE(scryfall_id, language, condition, is_foil)
   );
   ```
3. **Database module**: `src/db/mod.rs` with CRUD operations
4. **UI integration**: Add "Save to Database" button in Card Lookup
5. **Database path**: Use platform-specific data directory (same as cache)

#### Cost Estimate

| Component | Monthly Cost |
|-----------|--------------|
| Cloudflare R2 (< 10GB) | **$0** |
| Litestream | **$0** (open source) |
| SQLite | **$0** (embedded) |
| **Total** | **$0** |

#### References

- [Litestream Documentation](https://litestream.io/)
- [Litestream with S3-compatible Storage](https://litestream.io/guides/s3/)
- [Cloudflare R2 S3 Compatibility](https://developers.cloudflare.com/r2/api/s3/)
- [rusqlite Crate](https://docs.rs/rusqlite/)

---

## Code Review Findings & Technical Debt

*Last reviewed: 2026-02-12*

### Overall Assessment: Grade A- (85/100)

**Status**: ✅ **PRODUCTION READY**

The check_stock project demonstrates professional-grade Rust development with excellent architectural decisions, comprehensive testing (176 tests, all passing), and strong security practices. Zero clippy warnings, no unsafe code.

### High Priority Improvements

#### 1. Convert Blocking API Calls to Async
**Issue**: Uses `reqwest::blocking::Client` which freezes UI during network operations
- **Location**: [src/api/scryfall.rs:104](src/api/scryfall.rs#L104), [src/api/cardmarket.rs](src/api/cardmarket.rs)
- **Impact**: 50MB price guide download blocks entire app
- **Fix**: Replace with async client + Tokio runtime (pattern already exists in picking.rs)
- **Effort**: 2-3 days
- **Priority**: HIGH

#### 2. Implement Rate Limiting for Scryfall API
**Issue**: Semaphore limits concurrency but not requests/second (Scryfall limit: 10 req/s)
- **Location**: [src/ui/screens/picking.rs:105](src/ui/screens/picking.rs#L105)
- **Fix**: Add proper rate limiter (e.g., `governor` crate)
- **Effort**: 1 day
- **Priority**: MEDIUM

### Medium Priority Improvements

#### 3. Extract UI Business Logic for Testing
**Issue**: Large UI functions mix rendering + logic
- **Location**: [src/ui/screens/stock_checker.rs](src/ui/screens/stock_checker.rs) (480 lines)
- **Good Example**: picking.rs has 572 lines of tests
- **Fix**: Extract testable functions from UI screens
- **Effort**: 1-2 days
- **Priority**: MEDIUM

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

### Test Coverage

| Component | Coverage | Tests |
|-----------|----------|-------|
| Core Logic | ~95% | 127 tests |
| API Layer | ~80% | 19 tests |
| UI Screens | <5% | 1 screen tested (picking.rs) |
| Integration | - | 22 tests |
| **Total** | **176 tests** | **All passing** |

### Strengths to Maintain

- Excellent test fixtures (reused across tests)
- Clean error handling (custom ApiError enum)
- Perfect architecture adherence to CLAUDE.md
- Comprehensive performance tests
- Zero clippy warnings

---

## How to Contribute

If you have additional feature requests:

1. Create an issue in the repository
2. Use the "feature request" label
3. Provide detailed description and use case
4. Include mockups or examples if applicable
