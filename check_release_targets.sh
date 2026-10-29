#!/usr/bin/env bash
# Early warning for compile errors that only show up on the macOS / Windows
# release builds, without leaving Linux.
#
# Runs `cargo check --target <release target> --keep-going` for every
# non-Linux entry of the release build matrix (.github/workflows/release.yml).
#
# Limitation: crates that compile C code need the target's SDK, which Linux
# does not have. Their build scripts fail and are tolerated (KNOWN_C_CRATES);
# cargo then skips them and everything that depends on them (TLS, SQLite,
# HTTP and the apps themselves). Everything else is checked, including the
# platform-specific GUI code (winit, eframe, arboard, webbrowser, ...) where
# these errors usually come from. The full build runs on real macOS/Windows
# machines in the `release_targets` job of the Rust CI workflow.
#
# Usage: ./check_release_targets.sh            (all projects)
#        ./check_release_targets.sh check_stock

set -uo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# project:target pairs, as in the release build matrix (Linux is covered by
# the normal checks).
TARGETS=(
    "check_stock:aarch64-apple-darwin"
    "check_stock:x86_64-pc-windows-msvc"
    "accounting:x86_64-pc-windows-msvc"
)

# Build scripts that compile C and cannot run without the target's SDK.
KNOWN_C_CRATES=("ring" "libsqlite3-sys")

only_project="${1:-}"
failed=()
passed=()

is_known_c_crate() {
    local crate="$1"
    for known in "${KNOWN_C_CRATES[@]}"; do
        [[ "$crate" == "$known" ]] && return 0
    done
    return 1
}

for entry in "${TARGETS[@]}"; do
    project="${entry%%:*}"
    target="${entry#*:}"
    [[ -n "$only_project" && "$project" != "$only_project" ]] && continue

    echo ""
    echo "======================================"
    echo " $project → $target"
    echo "======================================"

    if ! rustup target list --installed | grep -qx "$target"; then
        echo "Installing Rust target $target..."
        if ! (cd "$SCRIPT_DIR/$project" && rustup target add "$target"); then
            echo -e "${RED}[ERROR]${NC} could not install target $target"
            failed+=("$project ($target)")
            continue
        fi
    fi

    log="$(mktemp)"
    (cd "$SCRIPT_DIR/$project" &&
        cargo check --target "$target" --all-targets --keep-going) >"$log" 2>&1

    ok=true

    # Rust compile errors: always a failure.
    if grep -q "^error: could not compile" "$log"; then
        ok=false
        grep -E "^error(\[E[0-9]+\])?:|^ *--> " "$log" | head -40
    fi

    # Build-script failures: only the known C crates are expected.
    while read -r crate; do
        if ! is_known_c_crate "$crate"; then
            ok=false
            echo -e "${RED}[ERROR]${NC} build script of '$crate' failed (not a known C crate):"
            grep -A3 "failed to run custom build command for \`$crate " "$log" | head -8
        fi
    done < <(sed -n 's/^error: failed to run custom build command for `\([^ ]*\) .*/\1/p' "$log")

    skipped=$(sed -n 's/^error: failed to run custom build command for `\([^ ]*\) .*/\1/p' "$log" | paste -sd, -)

    if $ok; then
        echo -e "${GREEN}[OK]${NC} no compile errors${skipped:+ (skipped C crates: $skipped and what depends on them)}"
        passed+=("$project ($target)")
    else
        echo -e "${RED}[FAILED]${NC} full log: $log"
        failed+=("$project ($target)")
        continue
    fi
    rm -f "$log"
done

echo ""
for p in "${passed[@]}"; do echo -e "  ${GREEN}✓${NC} $p"; done
for f in "${failed[@]}"; do echo -e "  ${RED}✗${NC} $f"; done

if [[ ${#failed[@]} -gt 0 ]]; then
    echo -e "${RED}Release-target check failed.${NC}"
    exit 1
fi
echo -e "${YELLOW}Note:${NC} crates with C code are only built on real macOS/Windows (CI job release_targets)."
