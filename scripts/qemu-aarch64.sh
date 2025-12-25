#!/bin/bash
# Run Splax OS on QEMU for aarch64
#
# Usage: ./scripts/qemu-aarch64.sh [options]
#   -d    Enable debugging (GDB server on port 1234)
#   -g    Enable graphics (use SDL display)
#   -m N  Set memory size in MB (default: 512)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
KERNEL_PATH="$PROJECT_ROOT/target/splax_kernel_aarch64/release/splax_kernel_aarch64.elf"

# Default options
MEMORY="512M"
DEBUG=""
DISPLAY_OPT="-nographic"
CPU="cortex-a72"

# Parse command line options
while getopts "dgm:" opt; do
    case $opt in
        d)
            DEBUG="-s -S"
            echo "Debugging enabled. Connect with: aarch64-linux-gnu-gdb -ex 'target remote :1234'"
            ;;
        g)
            DISPLAY_OPT="-display sdl"
            ;;
        m)
            MEMORY="${OPTARG}M"
            ;;
        \?)
            echo "Invalid option: -$OPTARG" >&2
            exit 1
            ;;
    esac
done

# Check if kernel exists
if [ ! -f "$KERNEL_PATH" ]; then
    echo "Kernel not found at: $KERNEL_PATH"
    echo "Building kernel..."
    cd "$PROJECT_ROOT"
    cargo kbuild-arm
fi

echo "Starting Splax OS on QEMU (aarch64)..."
echo "  CPU: $CPU"
echo "  Memory: $MEMORY"
echo "  Kernel: $KERNEL_PATH"
echo ""
echo "Press Ctrl+A X to exit QEMU"
echo ""

exec qemu-system-aarch64 \
    -M virt \
    -cpu "$CPU" \
    -m "$MEMORY" \
    -kernel "$KERNEL_PATH" \
    $DISPLAY_OPT \
    $DEBUG
