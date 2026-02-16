#!/bin/bash
set -euo pipefail

# Daily SQLite backup to Cloudflare R2 via rclone
# Setup:
#   1. Install rclone (v1.63+): curl https://rclone.org/install.sh | sudo bash
#   2. Configure rclone remote: rclone config
#   3. Copy backup.env.example to backup.env and fill in values
#   4. Test: sudo ./backup_to_r2.sh
#   5. Add to crontab (daily at 3 AM):
#      sudo crontab -e
#      0 3 * * * /path/to/backup_to_r2.sh >> /var/log/inventory_backup.log 2>&1

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ENV_FILE="${SCRIPT_DIR}/backup.env"

if [ ! -f "$ENV_FILE" ]; then
    echo "[$(date)] ERROR: ${ENV_FILE} not found. Copy backup.env.example and fill in values."
    exit 1
fi

# shellcheck source=/dev/null
source "$ENV_FILE"

# Validate required variables
for var in DB_PATH RCLONE_CONFIG R2_BUCKET; do
    if [ -z "${!var:-}" ]; then
        echo "[$(date)] ERROR: ${var} is not set in ${ENV_FILE}"
        exit 1
    fi
done

KEEP_DAYS="${KEEP_DAYS:-7}"
BACKUP_DIR="${BACKUP_DIR:-/tmp/inventory_backups}"
export RCLONE_CONFIG

mkdir -p "$BACKUP_DIR"

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_FILE="${BACKUP_DIR}/inventory_${TIMESTAMP}.sqlite"

echo "[$(date)] Starting backup..."

# Safe backup using sqlite3 .backup (transaction-safe)
sqlite3 "$DB_PATH" ".backup '${BACKUP_FILE}'"

gzip "$BACKUP_FILE"

# Upload to Cloudflare R2
rclone copy "${BACKUP_FILE}.gz" "$R2_BUCKET/"

# Rotate local backups
find "$BACKUP_DIR" -name "inventory_*.sqlite.gz" -mtime +"$KEEP_DAYS" -delete

# Rotate remote backups
rclone delete --min-age "${KEEP_DAYS}d" "$R2_BUCKET/"

echo "[$(date)] Backup complete: inventory_${TIMESTAMP}.sqlite.gz"
