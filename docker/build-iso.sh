#!/bin/bash
set -e

WORKSPACE=/workspace
KERNEL=$WORKSPACE/target/x86_64-unknown-none/release/splax_kernel
ISO_DIR=$WORKSPACE/target/iso
OUTPUT=$WORKSPACE/target/splax.iso

echo "=== Splax OS ISO Builder ==="

# Check if kernel exists
if [ ! -f "$KERNEL" ]; then
    echo "ERROR: Kernel not found at $KERNEL"
    echo "Please build the kernel first with:"
    echo "  cargo build -p splax_kernel --target x86_64-unknown-none -Z build-std=core,alloc --release"
    exit 1
fi

echo "Kernel found: $KERNEL"
echo "Size: $(ls -lh $KERNEL | awk '{print $5}')"

# Create ISO directory structure
rm -rf $ISO_DIR
mkdir -p $ISO_DIR/boot/grub

# Copy kernel
cp $KERNEL $ISO_DIR/boot/splax_kernel

# Create GRUB config
cat > $ISO_DIR/boot/grub/grub.cfg << 'EOF'
set timeout=3
set default=0

menuentry "Splax OS" {
    multiboot2 /boot/splax_kernel
    boot
}

menuentry "Splax OS (Serial Debug)" {
    multiboot2 /boot/splax_kernel debug
    boot
}
EOF

echo "Creating ISO..."

# Create the ISO
grub-mkrescue -o $OUTPUT $ISO_DIR 2>/dev/null

echo "=== ISO created successfully ==="
echo "Output: $OUTPUT"
echo "Size: $(ls -lh $OUTPUT | awk '{print $5}')"
echo ""
echo "Run with QEMU:"
echo "  qemu-system-x86_64 -cdrom target/splax.iso -serial stdio -no-reboot"
