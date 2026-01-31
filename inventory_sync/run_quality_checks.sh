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
echo " Code Quality Checks for inventory_sync"
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
echo -e "${YELLOW}[INFO]${NC} Running clippy (entire project)..."
if cargo clippy --all-targets --all-features -- -D warnings; then
    echo -e "${GREEN}[SUCCESS]${NC} clippy (entire project) passed!"
else
    echo -e "${RED}[ERROR]${NC} clippy (entire project) failed!"
    FAILED=1
fi
echo ""

# 3. Tests
echo "======================================"
echo " 3. Running Tests"
echo "======================================"
echo -e "${YELLOW}[INFO]${NC} Running all tests (unit, integration, doc)..."
if cargo test --verbose; then
    echo -e "${GREEN}[SUCCESS]${NC} all tests (unit, integration, doc) passed!"
else
    echo -e "${RED}[ERROR]${NC} tests failed!"
    FAILED=1
fi
echo ""

# 4. Doc tests
echo "======================================"
echo " 4. Documentation Tests"
echo "======================================"
echo -e "${YELLOW}[INFO]${NC} Running documentation tests..."
if cargo test --doc --verbose; then
    echo -e "${GREEN}[SUCCESS]${NC} documentation tests passed!"
else
    echo -e "${RED}[ERROR]${NC} documentation tests failed!"
    FAILED=1
fi
echo ""

# 5. Build check
echo "======================================"
echo " 5. Build Check"
echo "======================================"
echo -e "${YELLOW}[INFO]${NC} Running debug build..."
if cargo build; then
    echo -e "${GREEN}[SUCCESS]${NC} debug build passed!"
else
    echo -e "${RED}[ERROR]${NC} debug build failed!"
    FAILED=1
fi
echo ""

# 6. Release build
echo "======================================"
echo " 6. Release Build Check"
echo "======================================"
echo -e "${YELLOW}[INFO]${NC} Running release build..."
if cargo build --release; then
    echo -e "${GREEN}[SUCCESS]${NC} release build passed!"
else
    echo -e "${RED}[ERROR]${NC} release build failed!"
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
