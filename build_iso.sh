#!/bin/bash

set -euo pipefail

KERNEL="target/x86_64-unknown-none/release/sos"
ISO="target/sos.iso"
ISO_DIR="target/isofiles"
GRUB_CFG="grub/grub.cfg"

if ! command -v grub-mkrescue >/dev/null 2>&1; then
    echo "error: grub-mkrescue is required to build a bootable ISO."
    exit 1
fi

if ! command -v xorriso >/dev/null 2>&1; then
    echo "error: xorriso is required by grub-mkrescue to generate ISO images."
    echo "hint: install xorriso and rerun 'make run-iso'."
    exit 1
fi

if [ ! -f "$KERNEL" ]; then
    echo "Building S.O.S. kernel..."
    rustup target add x86_64-unknown-none 2>/dev/null || true
    cargo build --release --target x86_64-unknown-none
fi

mkdir -p "$ISO_DIR/boot/grub"
cp "$KERNEL" "$ISO_DIR/boot/sos"
cp "$GRUB_CFG" "$ISO_DIR/boot/grub/grub.cfg"

echo "Building bootable GRUB ISO..."
grub-mkrescue -o "$ISO" "$ISO_DIR" >/dev/null
echo "Created $ISO"
