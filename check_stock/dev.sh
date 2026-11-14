#!/usr/bin/env bash
# Run the web version of check_stock on this machine, to try it out before
# anything is deployed. Works on a COPY of the desktop app's database in
# ./dev-data (gitignored); the desktop file is only read, never changed.
#
#   ./dev.sh               web UI with hot reload on http://localhost:5173,
#                          server on 127.0.0.1:8080 (first start: copies the
#                          desktop DB and asks for a password for your login)
#   ./dev.sh --release     builds the UI into a release server and runs only
#                          that on http://127.0.0.1:8080 (what gets deployed)
#   ./dev.sh reset-data    fresh copy of the desktop data; local users and the
#                          audit log stay (the previous copy is kept as .bak)
#   ./dev.sh help
#
# Environment:
#   DESKTOP_DB    desktop database to copy
#                 (default: $XDG_DATA_HOME or ~/.local/share, then d2d_automations/inventory.db)
#   DEV_DATA_DIR  data folder (default: ./dev-data)
#   DEV_PORT      port (default: 8080)
#   DEV_ADMIN     login name of the local admin (default: your user name)
#   DEV_INVENTORY_SYNC_URL  price tracker for Cardmarket prices
#                 (default: http://cardscanner.local:3000, like the desktop
#                 app; empty = no prices)
#
# Needs Rust and Node.js 24 (web/.nvmrc). Both servers only listen on this
# machine, so nothing in dev-data is reachable from other machines. Ctrl-C
# stops everything.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

DESKTOP_DB="${DESKTOP_DB:-${XDG_DATA_HOME:-$HOME/.local/share}/d2d_automations/inventory.db}"
DATA_DIR="${DEV_DATA_DIR:-dev-data}"
PORT="${DEV_PORT:-8080}"
ADMIN="${DEV_ADMIN:-${USER:-admin}}"
INVENTORY_SYNC_URL="${DEV_INVENTORY_SYNC_URL-http://cardscanner.local:3000}"

usage() {
    # The comment block at the top of this file, without the '#'.
    awk 'NR == 1 { next } /^#/ { sub(/^# ?/, ""); print; next } { exit }' "$SCRIPT_DIR/dev.sh"
}

server() {
    local profile="$1"
    shift
    if [[ "$profile" == "release" ]]; then
        cargo run --quiet --release -p check_stock_server -- "$@"
    else
        cargo run --quiet -p check_stock_server -- "$@"
    fi
}

copy_desktop_db() {
    local profile="$1"
    if [[ ! -f "$DESKTOP_DB" ]]; then
        echo "No desktop database at $DESKTOP_DB." >&2
        echo "Set DESKTOP_DB to the desktop app's inventory.db." >&2
        exit 1
    fi
    echo "Copying $DESKTOP_DB into $DATA_DIR (the desktop file is not changed)..."
    if [[ -f "$DATA_DIR/inventory.db" ]]; then
        # Keeps users, sessions and the audit log; swaps only the desktop data.
        server "$profile" import-desktop --db "$DESKTOP_DB" --data-dir "$DATA_DIR" --refresh
    else
        server "$profile" import-desktop --db "$DESKTOP_DB" --data-dir "$DATA_DIR"
    fi
}

# On the first start, asks for a password for the local admin login (does
# nothing once any user exists). Three tries for a password that is too short.
ensure_admin() {
    local profile="$1"
    for _ in 1 2 3; do
        if server "$profile" create-admin --data-dir "$DATA_DIR" --username "$ADMIN" --if-none; then
            return 0
        fi
    done
    echo "No local admin created; run ./dev.sh again." >&2
    exit 1
}

require_node() {
    local want major
    want="$(cat web/.nvmrc)"
    if ! command -v node >/dev/null; then
        echo "Node.js $want is needed for the web UI (e.g. sudo snap install node --classic --channel=$want/stable)." >&2
        exit 1
    fi
    major="$(node --version 2>/dev/null | sed -E 's/^v([0-9]+).*/\1/')"
    if [[ "$major" != "$want" ]]; then
        echo "Node.js $want is needed for the web UI, found $(node --version 2>/dev/null)." >&2
        exit 1
    fi
}

# Installs the web UI's packages exactly as locked, when missing or outdated.
web_deps() {
    require_node
    if [[ ! -f web/node_modules/.package-lock.json || web/package-lock.json -nt web/node_modules/.package-lock.json ]]; then
        echo "Installing the web UI's packages (npm ci)..."
        (cd web && npm ci --no-fund)
    fi
}

prepare_data() {
    local profile="$1"
    if [[ ! -f "$DATA_DIR/inventory.db" ]]; then
        copy_desktop_db "$profile"
    fi
    ensure_admin "$profile"
}

# Server in the background, Vite (hot reload) in the foreground.
serve_dev() {
    web_deps
    prepare_data debug
    server debug serve --data-dir "$DATA_DIR" --bind "127.0.0.1:$PORT" \
        --inventory-sync-url "$INVENTORY_SYNC_URL" &
    local server_pid=$!
    trap 'kill "$server_pid" 2>/dev/null; wait "$server_pid" 2>/dev/null' EXIT
    echo "Open http://localhost:5173 (the API server starts on 127.0.0.1:$PORT; Ctrl-C stops both)"
    (cd web && CHECK_STOCK_API_PORT="$PORT" npm run dev)
}

# The UI built into a release server, exactly as deployed.
serve_release() {
    web_deps
    echo "Building the web UI..."
    (cd web && npm run build)
    prepare_data release
    echo "Open http://127.0.0.1:$PORT (Ctrl-C to stop)"
    server release serve --data-dir "$DATA_DIR" --bind "127.0.0.1:$PORT" \
        --inventory-sync-url "$INVENTORY_SYNC_URL"
}

case "${1:-}" in
    "") serve_dev ;;
    --release) serve_release ;;
    reset-data) copy_desktop_db debug ;;
    help | -h | --help) usage ;;
    *)
        usage >&2
        exit 1
        ;;
esac
