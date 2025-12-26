#!/bin/bash
# Run QEMU with serial output to file for debugging

cd "$(dirname "$0")/.."

rm -f /tmp/splax.log

# Run QEMU with serial output to file
# Using -monitor none to avoid any interactive prompts
qemu-system-x86_64 \
    -cdrom target/splax.iso \
    -serial file:/tmp/splax.log \
    -monitor none \
    -m 512M \
    -device virtio-net-pci,netdev=net0 \
    -netdev user,id=net0 \
    -display none \
    -no-reboot &

QPID=$!
echo "QEMU running with PID $QPID"

# Wait for OS to boot and run
sleep 15

# Kill QEMU
kill -9 $QPID 2>/dev/null
wait $QPID 2>/dev/null

# Show output
echo ""
echo "=== Serial Output ==="
cat /tmp/splax.log 2>/dev/null || echo "No log file created"
