#!/bin/bash

# Clippy script for d2d_automations check_stock project
# Usage: ./run_clippy.sh [options]

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Default options
STRICT_MODE=false
FIX_MODE=false
ALL_TARGETS=true
VERBOSE=false

# Function to print colored output
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

# Help function
show_help() {
    cat << EOF
Clippy Runner for d2d_automations check_stock

Usage: $0 [OPTIONS]

OPTIONS:
    -h, --help          Show this help message
    -s, --strict        Run clippy with warnings as errors (-D warnings)
    -f, --fix           Automatically fix issues where possible
    -t, --targets-only  Run only on main targets (no tests)
    -v, --verbose       Show verbose output
    -a, --all           Run comprehensive check (all targets, all features, strict)

EXAMPLES:
    $0                  # Basic clippy check
    $0 --strict         # Strict mode (warnings as errors)
    $0 --fix            # Fix automatically fixable issues
    $0 --all            # Comprehensive check
EOF
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_help
            exit 0
            ;;
        -s|--strict)
            STRICT_MODE=true
            shift
            ;;
        -f|--fix)
            FIX_MODE=true
            shift
            ;;
        -t|--targets-only)
            ALL_TARGETS=false
            shift
            ;;
        -v|--verbose)
            VERBOSE=true
            shift
            ;;
        -a|--all)
            STRICT_MODE=true
            ALL_TARGETS=true
            VERBOSE=true
            shift
            ;;
        *)
            print_error "Unknown option: $1"
            show_help
            exit 1
            ;;
    esac
done

# Change to project directory
cd "$(dirname "$0")"
print_status "Running clippy in $(pwd)"

# Build the clippy command
CLIPPY_CMD="cargo clippy"

if [ "$ALL_TARGETS" = true ]; then
    CLIPPY_CMD="$CLIPPY_CMD --all-targets --all-features"
fi

if [ "$FIX_MODE" = true ]; then
    CLIPPY_CMD="$CLIPPY_CMD --fix"
fi

if [ "$VERBOSE" = true ]; then
    CLIPPY_CMD="$CLIPPY_CMD --verbose"
fi

# Add strict mode flags
if [ "$STRICT_MODE" = true ]; then
    CLIPPY_CMD="$CLIPPY_CMD -- -D warnings"
fi

print_status "Running command: $CLIPPY_CMD"
echo

# Run clippy
if eval "$CLIPPY_CMD"; then
    print_success "Clippy check completed successfully!"
    
    # Show summary
    echo
    print_status "Summary:"
    echo "  ✅ No clippy warnings found"
    if [ "$STRICT_MODE" = true ]; then
        echo "  ✅ Strict mode: warnings treated as errors"
    fi
    if [ "$ALL_TARGETS" = true ]; then
        echo "  ✅ Checked all targets (lib, bin, tests)"
    fi
    if [ "$FIX_MODE" = true ]; then
        echo "  ✅ Auto-fixed issues where possible"
    fi
else
    print_error "Clippy found issues!"
    echo
    print_warning "To fix automatically fixable issues, run:"
    echo "  $0 --fix"
    echo
    print_warning "For strict checking (warnings as errors), run:"
    echo "  $0 --strict"
    exit 1
fi
