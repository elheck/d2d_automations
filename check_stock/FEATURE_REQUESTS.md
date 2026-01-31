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

* Checklist for picking

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

## How to Contribute

If you have additional feature requests:

1. Create an issue in the repository
2. Use the "feature request" label
3. Provide detailed description and use case
4. Include mockups or examples if applicable
