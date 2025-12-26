#!/bin/bash
# Splax OS QEMU Runner
#
# Usage:
#   ./scripts/qemu.sh [options]
#
# Options:
#   --debug     - Enable GDB debugging (wait for debugger)
#   --kvm       - Enable KVM acceleration (Linux only)
#   --iso       - Boot from ISO instead of UEFI disk image
#   --build     - Build before running
#   --no-gui    - Run without display (serial only)
#
# Environment:
#   OVMF_OVERRIDE - Override OVMF firmware path

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT"

# Configuration
KERNEL_BIN="target/x86_64-unknown-none/release/splax_kernel"
DISK_IMG="target/splax.img"
ISO_IMG="target/splax.iso"

# Defaults
DEBUG=""
KVM=""
USE_ISO=true
DO_BUILD=false
NO_GUI=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --debug)
            DEBUG="-s -S"
            echo "==> Debug mode: Waiting for GDB on localhost:1234"
            shift
            ;;
        --kvm)
            KVM="-enable-kvm"
            shift
            ;;
        --iso)
            USE_ISO=true
            shift
            ;;
        --build)
            DO_BUILD=true
            shift
            ;;
        --no-gui)
            NO_GUI=true
            shift
            ;;
        -h|--help)
            echo "Splax OS QEMU Runner"
            echo ""
            echo "Usage: ./scripts/qemu.sh [options]"
            echo ""
            echo "Options:"
            echo "  --debug     Enable GDB debugging (wait for debugger)"
            echo "  --kvm       Enable KVM acceleration (Linux only)"
            echo "  --iso       Boot from ISO (requires ./scripts/build-docker-iso.sh first)"
            echo "  --build     Build kernel before running"
            echo "  --no-gui    Run without display (serial only)"
            echo ""
            echo "Environment:"
            echo "  OVMF_OVERRIDE - Override OVMF firmware path"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            echo "Use --help for usage information"
            exit 1
            ;;
    esac
done

# Build if requested
if [[ "$DO_BUILD" == true ]]; then
    echo "==> Building Kernel..."
    cargo build -p splax_kernel --bin splax_kernel --target x86_64-unknown-none --release
fi

# ISO boot mode
if [[ "$USE_ISO" == true ]]; then
    if [[ ! -f "$ISO_IMG" ]]; then
        echo "Error: ISO not found at $ISO_IMG"
        echo "Build with: ./scripts/build-docker-iso.sh"
        exit 1
    fi
    
    QEMU_ARGS=(
        "-cdrom" "$ISO_IMG"
        "-serial" "stdio"
        "-m" "512M"
        # Network: virtio-net with user-mode networking
        "-device" "virtio-net-pci,netdev=net0"
        "-netdev" "user,id=net0,hostfwd=tcp::8080-:80"
    )
    
    if [[ "$NO_GUI" == true ]]; then
        QEMU_ARGS+=("-display" "none")
    fi
    
    if [[ -n "$KVM" ]]; then
        QEMU_ARGS+=("$KVM")
    fi
    
    if [[ -n "$DEBUG" ]]; then
        QEMU_ARGS+=($DEBUG)
    fi
    
    echo "==> Launching Splax OS from ISO..."
    qemu-system-x86_64 "${QEMU_ARGS[@]}"
    exit 0
fi

# UEFI boot mode - Find OVMF firmware
if [ -n "$OVMF_OVERRIDE" ]; then
    OVMF_CODE="$OVMF_OVERRIDE"
    OVMF_VARS=""
    echo "Using user-specified OVMF firmware: $OVMF_CODE"
else
    OVMF_CODE=""
    OVMF_VARS=""
    # Common paths for Homebrew (macOS) and Linux
    POSSIBLE_PATHS=(
        "/opt/homebrew/share/qemu/edk2-x86_64-code.fd"   # Apple Silicon Brew
        "/usr/local/share/qemu/edk2-x86_64-code.fd"      # Intel Mac Brew
        "/usr/share/OVMF/OVMF_CODE.fd"                   # Debian/Ubuntu
        "/usr/share/qemu/OVMF.fd"                        # Arch/Fedora
        "/usr/share/edk2/ovmf/OVMF_CODE.fd"              # Other Linux
    )
    for path in "${POSSIBLE_PATHS[@]}"; do
        if [ -f "$path" ]; then
            OVMF_CODE="$path"
            # Try to find corresponding VARS file
            if [[ "$path" == *"edk2-x86_64-code.fd"* ]]; then
                 if [ -f "${path/code.fd/vars.fd}" ]; then
                     OVMF_VARS="${path/code.fd/vars.fd}"
                 fi
            elif [[ "$path" == *"OVMF_CODE.fd"* ]]; then
                 VARS_PATH="${path/CODE/VARS}"
                 if [ -f "$VARS_PATH" ]; then
                     OVMF_VARS="$VARS_PATH"
                 fi
            fi
            echo "Found OVMF firmware: $OVMF_CODE"
            break
        fi
    done
fi

# Check if OVMF was found for UEFI boot
if [ -z "$OVMF_CODE" ]; then
    echo "Warning: OVMF firmware not found for UEFI boot"
    echo "Falling back to ISO boot mode..."
    
    if [[ ! -f "$ISO_IMG" ]]; then
        echo "Error: Neither OVMF nor ISO found"
        echo "Install OVMF: brew install qemu (includes OVMF)"
        echo "Or build ISO: ./scripts/build-docker-iso.sh"
        exit 1
    fi
    
    QEMU_ARGS=(
        "-cdrom" "$ISO_IMG"
        "-serial" "stdio"
        "-m" "512M"
    )
    
    if [[ "$NO_GUI" == true ]]; then
        QEMU_ARGS+=("-display" "none")
    fi
    
    echo "==> Launching Splax OS from ISO (fallback)..."
    qemu-system-x86_64 "${QEMU_ARGS[@]}"
    exit 0
fi

# Check kernel exists
if [[ ! -f "$KERNEL_BIN" ]]; then
    echo "Error: Kernel not found at $KERNEL_BIN"
    echo "Build with: cargo build -p splax_kernel --bin splax_kernel --target x86_64-unknown-none --release"
    echo "Or use: ./scripts/qemu.sh --build"
    exit 1
fi

# Create FAT32 disk image for UEFI boot
echo "==> Creating UEFI disk image..."
dd if=/dev/zero of=$DISK_IMG bs=1M count=64 2>/dev/null

# Check if mkfs.fat is available (Linux) or use newfs_msdos (macOS)
if command -v mkfs.fat &> /dev/null; then
    mkfs.fat -F 32 $DISK_IMG
elif command -v newfs_msdos &> /dev/null; then
    newfs_msdos -F 32 $DISK_IMG
else
    echo "Error: No FAT32 formatter found (mkfs.fat or newfs_msdos)"
    exit 1
fi

# Mount and copy using mtools (avoids sudo)
if ! command -v mmd &> /dev/null; then
    echo "Error: mtools not found. Install with: brew install mtools"
    exit 1
fi

mmd -i $DISK_IMG ::/EFI 2>/dev/null || true
mmd -i $DISK_IMG ::/EFI/BOOT 2>/dev/null || true
mmd -i $DISK_IMG ::/boot 2>/dev/null || true

# Copy kernel
mcopy -i $DISK_IMG $KERNEL_BIN ::/boot/splax_kernel

# Prepare QEMU arguments
QEMU_ARGS=(
    "-machine" "q35"
    "-cpu" "qemu64"
    "-m" "512M"
    "-drive" "if=pflash,format=raw,readonly=on,file=$OVMF_CODE"
    "-drive" "format=raw,file=$DISK_IMG"
    "-serial" "stdio"
    "-net" "none"
)

if [ -n "$OVMF_VARS" ]; then
    QEMU_ARGS+=("-drive" "if=pflash,format=raw,file=$OVMF_VARS")
fi

if [[ "$NO_GUI" == true ]]; then
    QEMU_ARGS+=("-display" "none")
else
    QEMU_ARGS+=("-vga" "std")
fi

if [[ -n "$KVM" ]]; then
    QEMU_ARGS+=("$KVM")
fi

if [[ -n "$DEBUG" ]]; then
    QEMU_ARGS+=($DEBUG)
fi

# Run QEMU
echo "==> Launching Splax OS (UEFI mode)..."
qemu-system-x86_64 "${QEMU_ARGS[@]}"
