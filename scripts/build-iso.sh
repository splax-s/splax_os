#!/bin/bash
# Build and run Splax OS with Limine bootloader
#
# This script creates a bootable ISO using the Limine bootloader

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_ROOT/target/iso"
LIMINE_DIR="$BUILD_DIR/limine"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}==> Building Splax OS...${NC}"

cd "$PROJECT_ROOT"

# Build kernel
cargo build -p splax_kernel --bin splax_kernel --target x86_64-unknown-none --release 2>&1

KERNEL="target/x86_64-unknown-none/release/splax_kernel"

if [[ ! -f "$KERNEL" ]]; then
    echo -e "${RED}Error: Kernel not found${NC}"
    exit 1
fi

# Create build directory
mkdir -p "$BUILD_DIR/iso_root/boot/limine"
mkdir -p "$BUILD_DIR/iso_root/EFI/BOOT"

# Download Limine if not present
if [[ ! -d "$LIMINE_DIR" ]]; then
    echo -e "${YELLOW}==> Downloading Limine bootloader...${NC}"
    git clone --depth 1 --branch v8.x-binary https://github.com/limine-bootloader/limine.git "$LIMINE_DIR"
fi

# Copy kernel
cp "$KERNEL" "$BUILD_DIR/iso_root/boot/splax_kernel"

# Create Limine config
cat > "$BUILD_DIR/iso_root/boot/limine/limine.conf" << 'EOF'
# Limine configuration for Splax OS

timeout: 3
default_entry: 1

/Splax OS
    protocol: limine
    kernel_path: boot():/boot/splax_kernel
EOF

# Copy Limine files
cp "$LIMINE_DIR/limine-bios.sys" "$BUILD_DIR/iso_root/boot/limine/"
cp "$LIMINE_DIR/limine-bios-cd.bin" "$BUILD_DIR/iso_root/boot/limine/"
cp "$LIMINE_DIR/limine-uefi-cd.bin" "$BUILD_DIR/iso_root/boot/limine/"
cp "$LIMINE_DIR/BOOT*.EFI" "$BUILD_DIR/iso_root/EFI/BOOT/" 2>/dev/null || true
cp "$LIMINE_DIR/BOOTX64.EFI" "$BUILD_DIR/iso_root/EFI/BOOT/" 2>/dev/null || true
cp "$LIMINE_DIR/BOOTIA32.EFI" "$BUILD_DIR/iso_root/EFI/BOOT/" 2>/dev/null || true

echo -e "${GREEN}==> Files prepared for ISO${NC}"
echo -e "${YELLOW}ISO creation requires xorriso (brew install xorriso)${NC}"

# Try to create ISO if xorriso is available
if command -v xorriso &> /dev/null; then
    echo -e "${GREEN}==> Creating ISO...${NC}"
    xorriso -as mkisofs \
        -b boot/limine/limine-bios-cd.bin \
        -no-emul-boot -boot-load-size 4 -boot-info-table \
        --efi-boot boot/limine/limine-uefi-cd.bin \
        -efi-boot-part --efi-boot-image --protective-msdos-label \
        "$BUILD_DIR/iso_root" \
        -o "$BUILD_DIR/splax.iso"
    
    # Install Limine
    "$LIMINE_DIR/limine" bios-install "$BUILD_DIR/splax.iso"
    
    echo -e "${GREEN}==> ISO created: $BUILD_DIR/splax.iso${NC}"
    echo -e "${YELLOW}Run with: qemu-system-x86_64 -cdrom $BUILD_DIR/splax.iso -serial stdio${NC}"
else
    echo -e "${YELLOW}xorriso not found - ISO creation skipped${NC}"
    echo -e "${YELLOW}Install with: brew install xorriso${NC}"
fi
