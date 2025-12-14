# Feature Requests

This document contains feature requests for the Check Stock Application.

## High Priority

### Database Integration with Off-Site Backup

**Goal**: Store cards added via Card Lookup in a local SQLite database with automatic off-site backup to Cloudflare R2.

#### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Local Machine (On-Prem)                  │
│  ┌─────────────┐    ┌──────────────┐    ┌────────────────┐  │
│  │ Rust App    │───▶│ SQLite DB    │───▶│ rclone         │  │
│  │ (check_stock)│    │ (cards.db)   │    │ (cron/manual)  │  │
│  └─────────────┘    └──────────────┘    └───────┬────────┘  │
└─────────────────────────────────────────────────┼───────────┘
                                                  │
                                                  ▼ Sync on schedule
                                    ┌─────────────────────────┐
                                    │  Cloudflare R2 (Free)    │
                                    │  - 10 GB storage         │
                                    │  - No egress fees        │
                                    │  - S3-compatible API     │
                                    └─────────────────────────┘
```

#### Why SQLite + Cloudflare R2 + rclone?

- **SQLite**: Single-file database, zero config, handles millions of cards easily
- **Cloudflare R2**: 10GB free tier, no egress fees, S3-compatible
- **rclone**: Simple, reliable file sync to 70+ cloud providers

#### Setup Requirements

##### 1. Cloudflare R2 Setup
1. Create Cloudflare account (free)
2. Enable R2 in dashboard
3. Create a bucket (e.g., `mtg-card-backup`)
4. Generate R2 API token with read/write permissions
5. Note the Account ID, Access Key ID, and Secret Access Key

##### 2. rclone Installation
```bash
# Linux
curl https://rclone.org/install.sh | sudo bash

# macOS
brew install rclone

# Verify
rclone version
```

##### 3. rclone Configuration
```bash
rclone config

# Follow prompts:
# n) New remote
# name> r2
# Storage> s3
# provider> Cloudflare
# access_key_id> <R2_ACCESS_KEY_ID>
# secret_access_key> <R2_SECRET_ACCESS_KEY>
# endpoint> https://<ACCOUNT_ID>.r2.cloudflarestorage.com
# (accept defaults for rest)
```

Or create `~/.config/rclone/rclone.conf` directly:
```ini
[r2]
type = s3
provider = Cloudflare
access_key_id = <R2_ACCESS_KEY_ID>
secret_access_key = <R2_SECRET_ACCESS_KEY>
endpoint = https://<ACCOUNT_ID>.r2.cloudflarestorage.com
```

##### 4. Backup Script
Create `~/scripts/backup-cards-db.sh`:
```bash
#!/bin/bash
set -e

DB_PATH="$HOME/.local/share/d2d_automations/cards.db"
BACKUP_NAME="cards-$(date +%Y%m%d-%H%M%S).db"

# Create safe backup copy (SQLite online backup)
sqlite3 "$DB_PATH" ".backup /tmp/$BACKUP_NAME"

# Upload to R2
rclone copy "/tmp/$BACKUP_NAME" r2:mtg-card-backup/

# Keep last 7 daily backups on R2
rclone delete r2:mtg-card-backup/ --min-age 7d

# Cleanup
rm "/tmp/$BACKUP_NAME"

echo "Backup complete: $BACKUP_NAME"
```

```bash
chmod +x ~/scripts/backup-cards-db.sh
```

##### 5. Automate with Cron
```bash
# Edit crontab
crontab -e

# Add daily backup at 2am
0 2 * * * /home/user/scripts/backup-cards-db.sh >> /var/log/cards-backup.log 2>&1
```

##### 6. Restore from Backup
```bash
# List available backups
rclone ls r2:mtg-card-backup/

# Download specific backup
rclone copy r2:mtg-card-backup/cards-20251214-020000.db /tmp/

# Restore (stop app first)
cp /tmp/cards-20251214-020000.db ~/.local/share/d2d_automations/cards.db
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
| rclone | **$0** (open source) |
| SQLite | **$0** (embedded) |
| **Total** | **$0** |

#### References

- [rclone Documentation](https://rclone.org/docs/)
- [rclone with Cloudflare R2](https://rclone.org/s3/#cloudflare-r2)
- [Cloudflare R2 S3 Compatibility](https://developers.cloudflare.com/r2/api/s3/)
- [rusqlite Crate](https://docs.rs/rusqlite/)

---

## How to Contribute

If you have additional feature requests:

1. Create an issue in the repository
2. Use the "feature request" label
3. Provide detailed description and use case
4. Include mockups or examples if applicable
