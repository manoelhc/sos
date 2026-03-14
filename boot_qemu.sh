#!/bin/bash
# QEMU Boot Script for S.O.S.

set -e

KERNEL="target/x86_64-unknown-none/release/sos"

if [ ! -f "$KERNEL" ]; then
    echo "Building S.O.S. kernel..."
    rustup target add x86_64-unknown-none 2>/dev/null || true
    cargo build --release --target x86_64-unknown-none
fi

echo "Booting QEMU..."
qemu-system-x86_64 \
    -kernel "$KERNEL" \
    -m 128M \
    -nographic \
    -monitor none \
    -no-reboot \
    -serial file:serial.log \
    "$@"
