#!/usr/bin/env bash
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Get the directory of this script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "======================================"
echo " Code Quality Checks for mtg_common"
echo "======================================"
echo "Project directory: $SCRIPT_DIR"
echo ""

# Track if any check fails
FAILED=0

# 1. Format check
echo "======================================"
echo " 1. Code Formatting Check"
echo "======================================"
echo -e "${YELLOW}[INFO]${NC} Running cargo fmt check..."
if cargo fmt --all -- --check; then
    echo -e "${GREEN}[SUCCESS]${NC} cargo fmt check passed!"
else
    echo -e "${RED}[ERROR]${NC} cargo fmt check failed!"
    FAILED=1
fi
echo ""

# 2. Clippy
echo "======================================"
echo " 2. Clippy Linting"
echo "======================================"
echo -e "${YELLOW}[INFO]${NC} Running clippy (all features)..."
if cargo clippy --all-targets --all-features -- -D warnings; then
    echo -e "${GREEN}[SUCCESS]${NC} clippy (all features) passed!"
else
    echo -e "${RED}[ERROR]${NC} clippy (all features) failed!"
    FAILED=1
fi
echo ""

# 3. Tests
echo "======================================"
echo " 3. Running Tests"
echo "======================================"
echo -e "${YELLOW}[INFO]${NC} Running all tests (all features)..."
if cargo test --verbose --all-features; then
    echo -e "${GREEN}[SUCCESS]${NC} all tests passed!"
else
    echo -e "${RED}[ERROR]${NC} tests failed!"
    FAILED=1
fi
echo ""

# 4. Build check
echo "======================================"
echo " 4. Build Check"
echo "======================================"
echo -e "${YELLOW}[INFO]${NC} Running debug build (all features)..."
if cargo build --all-features; then
    echo -e "${GREEN}[SUCCESS]${NC} debug build passed!"
else
    echo -e "${RED}[ERROR]${NC} debug build failed!"
    FAILED=1
fi
echo ""

# Summary
echo "======================================"
echo " Summary"
echo "======================================"
if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}All checks passed!${NC}"
    exit 0
else
    echo -e "${RED}Some checks failed!${NC}"
    exit 1
fi
