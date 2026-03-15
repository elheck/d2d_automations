#!/usr/bin/env bash
set -uo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECTS=("check_stock" "accounting" "inventory_sync")

failed=()
passed=()

for project in "${PROJECTS[@]}"; do
    script="$SCRIPT_DIR/$project/run_quality_checks.sh"
    if [[ ! -x "$script" ]]; then
        echo -e "${YELLOW}[SKIP]${NC} $project — no run_quality_checks.sh found"
        continue
    fi

    echo ""
    echo "======================================"
    echo " Running quality checks: $project"
    echo "======================================"

    if (cd "$SCRIPT_DIR/$project" && ./run_quality_checks.sh); then
        passed+=("$project")
    else
        failed+=("$project")
    fi
done

echo ""
echo "======================================"
echo " Overall Summary"
echo "======================================"
for p in "${passed[@]}"; do
    echo -e "  ${GREEN}✓${NC} $p"
done
for f in "${failed[@]}"; do
    echo -e "  ${RED}✗${NC} $f"
done
echo ""

if [[ ${#failed[@]} -gt 0 ]]; then
    echo -e "${RED}Some projects failed!${NC}"
    exit 1
else
    echo -e "${GREEN}All projects passed!${NC}"
fi
