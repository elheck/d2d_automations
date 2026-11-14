#!/usr/bin/env bash
# Run the web version of check_stock on this machine, to try it out before
# anything is deployed. Works on a COPY of the desktop app's database in
# ./dev-data (gitignored); the desktop file is only read, never changed.
#
#   ./dev.sh               start the server on http://127.0.0.1:8080
#                          (copies the desktop DB first if dev-data is empty)
#   ./dev.sh --release     same with a release build (what gets deployed)
#   ./dev.sh reset-data    replace dev-data with a fresh copy of the desktop DB
#                          (the previous copy is kept as a .bak file)
#   ./dev.sh help
#
# Environment:
#   DESKTOP_DB    desktop database to copy
#                 (default: $XDG_DATA_HOME or ~/.local/share, then d2d_automations/inventory.db)
#   DEV_DATA_DIR  data folder (default: ./dev-data)
#   DEV_PORT      port (default: 8080)
#
# The server only listens on 127.0.0.1, so nothing in dev-data is reachable
# from other machines. Ctrl-C stops it.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

DESKTOP_DB="${DESKTOP_DB:-${XDG_DATA_HOME:-$HOME/.local/share}/d2d_automations/inventory.db}"
DATA_DIR="${DEV_DATA_DIR:-dev-data}"
PORT="${DEV_PORT:-8080}"

usage() {
    sed -n '2,20p' "$SCRIPT_DIR/dev.sh" | sed 's/^# \{0,1\}//'
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
    server "$profile" import-desktop --db "$DESKTOP_DB" --data-dir "$DATA_DIR" --replace
}

serve() {
    local profile="$1"
    if [[ ! -f "$DATA_DIR/inventory.db" ]]; then
        copy_desktop_db "$profile"
    fi
    echo "Starting check_stock server on http://127.0.0.1:$PORT (Ctrl-C to stop)"
    server "$profile" serve --data-dir "$DATA_DIR" --bind "127.0.0.1:$PORT"
}

case "${1:-}" in
    "") serve debug ;;
    --release) serve release ;;
    reset-data) copy_desktop_db debug ;;
    help | -h | --help) usage ;;
    *)
        usage >&2
        exit 1
        ;;
esac
