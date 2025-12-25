#!/bin/bash
# Splax OS Build Script
#
# Usage:
#   ./scripts/build.sh [target] [options]
#
# Targets:
#   x86_64    - Build for x86_64 (default)
#   aarch64   - Build for AArch64
#   all       - Build for all targets
#
# Options:
#   --release - Build in release mode
#   --clean   - Clean before building

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT"

# Defaults
TARGET="x86_64"
RELEASE=""
CLEAN=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        x86_64|aarch64|all)
            TARGET="$1"
            shift
            ;;
        --release)
            RELEASE="--release"
            shift
            ;;
        --clean)
            CLEAN="1"
            shift
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Clean if requested
if [[ -n "$CLEAN" ]]; then
    echo "==> Cleaning..."
    cargo clean
fi

# Build function
build_target() {
    local target=$1
    local target_json=""
    
    case $target in
        x86_64)
            target_json="splax_kernel.json"
            ;;
        aarch64)
            target_json="splax_kernel_aarch64.json"
            ;;
    esac
    
    echo "==> Building for $target..."
    cargo build --target "$target_json" $RELEASE -Z build-std=core,alloc
}

# Build
case $TARGET in
    x86_64)
        build_target x86_64
        ;;
    aarch64)
        build_target aarch64
        ;;
    all)
        build_target x86_64
        build_target aarch64
        ;;
esac

echo "==> Build complete!"
