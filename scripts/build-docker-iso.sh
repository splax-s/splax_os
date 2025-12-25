#!/bin/bash
# Create bootable ISO using Docker (for macOS compatibility)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT"

# Build kernel first
echo "==> Building kernel..."
cargo build -p splax_kernel --bin splax_kernel --target x86_64-unknown-none --release

KERNEL="target/x86_64-unknown-none/release/splax_kernel"
if [[ ! -f "$KERNEL" ]]; then
    echo "Error: Kernel not found"
    exit 1
fi

# Build Docker image if needed
echo "==> Setting up Docker build environment..."
docker build -t splax-grub -f docker/Dockerfile.grub docker/

# Create ISO directory structure
mkdir -p target/iso/boot/grub

# Copy kernel
cp "$KERNEL" target/iso/boot/splax_kernel

# Create GRUB config
cat > target/iso/boot/grub/grub.cfg << 'EOF'
set timeout=3
set default=0

menuentry "Splax OS" {
    multiboot2 /boot/splax_kernel
    boot
}
EOF

# Create ISO using Docker
echo "==> Creating ISO..."
docker run --rm -v "$PROJECT_ROOT/target/iso:/build/iso" -v "$PROJECT_ROOT/target:/build/output" splax-grub \
    grub-mkrescue -o /build/output/splax.iso /build/iso

echo "==> ISO created: target/splax.iso"
echo "Run with: qemu-system-x86_64 -cdrom target/splax.iso -serial stdio"
