#!/bin/bash

# Comprehensive code quality script for d2d_automations accounting (sevdesk_invoicing)
# This script runs multiple code quality checks: clippy, formatting, tests, etc.

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Function to print colored output
print_header() {
    echo -e "${CYAN}======================================${NC}"
    echo -e "${CYAN} $1${NC}"
    echo -e "${CYAN}======================================${NC}"
}

print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Track overall success
OVERALL_SUCCESS=true

# Function to run a command and track success
run_check() {
    local name="$1"
    local cmd="$2"
    
    print_status "Running $name..."
    
    if eval "$cmd"; then
        print_success "$name passed!"
        return 0
    else
        print_error "$name failed!"
        OVERALL_SUCCESS=false
        return 1
    fi
}

# Change to project directory
cd "$(dirname "$0")"
print_header "Code Quality Checks for $(basename $(pwd))"
echo "Project directory: $(pwd)"
echo

# 1. Code Formatting Check
print_header "1. Code Formatting Check"
run_check "cargo fmt check" "cargo fmt --all -- --check"
echo

# 2. Clippy Linting
print_header "2. Clippy Linting"
run_check "clippy (entire project)" "cargo clippy --all-targets --all-features -- -D warnings"
echo

# 3. Run Unit Tests
print_header "3. Running Unit Tests"
run_check "unit tests" "cargo test --lib"
echo

# 4. Run Integration Tests
print_header "4. Running Integration Tests"
run_check "integration tests" "cargo test --test '*'"
echo

# 5. Documentation Tests
print_header "5. Documentation Tests"
run_check "documentation tests" "cargo test --doc"
echo

# 6. Build Check
print_header "6. Build Check"
run_check "debug build" "cargo build"
echo

# 7. Release Build Check
print_header "7. Release Build Check"
run_check "release build" "cargo build --release"
echo

# 8. Security Audit (optional - only if cargo-audit is installed)
print_header "8. Security Audit"
if command -v cargo-audit &> /dev/null; then
    run_check "security audit" "cargo audit"
else
    print_warning "cargo-audit not installed. Install with: cargo install cargo-audit"
fi
echo

# Final Summary
print_header "SUMMARY"

if [ "$OVERALL_SUCCESS" = true ]; then
    print_success "All code quality checks passed! ✨"
    echo
    echo "Your code is ready for:"
    echo "  ✅ Commit and push"
    echo "  ✅ Pull request"
    echo "  ✅ Production deployment"
    echo
    exit 0
else
    print_error "Some checks failed!"
    echo
    echo "Please fix the issues above before:"
    echo "  ❌ Committing code"
    echo "  ❌ Creating pull requests"
    echo "  ❌ Deploying to production"
    echo
    echo "Quick fixes:"
    echo "  📝 Format code: cargo fmt"
    echo "  🔧 Fix clippy issues: cargo clippy --fix --allow-dirty"
    echo "  🧪 Run tests: cargo test"
    echo
    exit 1
fi
