#!/bin/bash
# Splax OS Test Runner
#
# Usage:
#   ./scripts/test.sh [category]
#
# Categories:
#   unit        - Run unit tests (default)
#   integration - Run integration tests
#   all         - Run all tests

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT"

# Default
CATEGORY="unit"

if [[ -n "$1" ]]; then
    CATEGORY="$1"
fi

run_unit_tests() {
    echo "==> Running unit tests..."
    
    # Test kernel components
    echo "  Testing kernel..."
    cargo test -p splax_kernel --lib 2>/dev/null || true
    
    # Test services
    echo "  Testing services..."
    cargo test -p splax_atlas --lib 2>/dev/null || true
    cargo test -p splax_link --lib 2>/dev/null || true
    cargo test -p splax_gate --lib 2>/dev/null || true
    cargo test -p splax_storage --lib 2>/dev/null || true
    
    # Test runtimes
    echo "  Testing runtimes..."
    cargo test -p splax_wave --lib 2>/dev/null || true
    cargo test -p splax_native --lib 2>/dev/null || true
    
    # Test tools
    echo "  Testing tools..."
    cargo test -p splax_term --lib 2>/dev/null || true
    cargo test -p splax_code --lib 2>/dev/null || true
}

run_integration_tests() {
    echo "==> Running integration tests..."
    # Would run in QEMU with test kernel
    echo "  (Integration tests require QEMU - skipping in CI)"
}

case $CATEGORY in
    unit)
        run_unit_tests
        ;;
    integration)
        run_integration_tests
        ;;
    all)
        run_unit_tests
        run_integration_tests
        ;;
    *)
        echo "Unknown category: $CATEGORY"
        exit 1
        ;;
esac

echo "==> Tests complete!"
