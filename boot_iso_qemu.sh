#!/bin/bash

set -euo pipefail

ISO="target/sos.iso"

if [ ! -f "$ISO" ]; then
    echo "ISO not found, building it first..."
    ./build_iso.sh
fi

echo "Booting QEMU from ISO..."
qemu-system-x86_64 \
    -cdrom "$ISO" \
    -boot d \
    -m 128M \
    -display none \
    -vnc :0 \
    -no-reboot \
    -serial file:serial.log \
    "$@"
