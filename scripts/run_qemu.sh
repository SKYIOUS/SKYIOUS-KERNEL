#!/bin/bash
# run_qemu.sh — Boot the Vahi kernel in QEMU.
#
# Usage:
#   ./scripts/run_qemu.sh                  # UEFI boot with OVMF
#   ./scripts/run_qemu.sh --bios           # Legacy BIOS boot
#   ./scripts/run_qemu.sh --display        # Show display (VGA)
#   ./scripts/run_qemu.sh --debug          # Enable GDB stub on :1234
#   ./scripts/run_qemu.sh --custom PATH    # Boot a specific disk image
#
# Press Ctrl+A, X to exit QEMU.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
IMAGE="$ROOT_DIR/bootimage-vahi_kernel.bin"
BIOS_BOOT=0
SHOW_DISPLAY=0
DEBUG=0
CUSTOM_IMAGE=""

for arg in "$@"; do
    case "$arg" in
        --bios)       BIOS_BOOT=1 ;;
        --display)    SHOW_DISPLAY=1 ;;
        --debug)      DEBUG=1 ;;
        --custom)     CUSTOM_IMAGE="$2"; shift ;;
        -h|--help)
            echo "Usage: $0 [--bios] [--display] [--debug] [--custom PATH]"
            exit 0
            ;;
    esac
done

if [ -n "$CUSTOM_IMAGE" ]; then
    IMAGE="$CUSTOM_IMAGE"
fi

if [ ! -f "$IMAGE" ]; then
    echo "ERROR: Disk image not found at $IMAGE"
    echo "Run ./scripts/build.sh first."
    exit 1
fi

# Build QEMU command
QEMU=(qemu-system-x86_64)
QEMU+=(-m 512M)
QEMU+=(-serial stdio)
QEMU+=(-drive "file=$IMAGE,format=raw")

if [ "$BIOS_BOOT" -eq 1 ]; then
    echo "Booting in BIOS mode..."
else
    # UEFI boot with OVMF
    OVMF="$ROOT_DIR/OVMF.fd"
    if [ -f "$OVMF" ]; then
        QEMU+=(-drive "if=pflash,format=raw,readonly=on,file=$OVMF")
        echo "Booting in UEFI mode..."
    else
        echo "WARNING: OVMF.fd not found, falling back to BIOS mode."
        BIOS_BOOT=1
    fi
fi

if [ "$SHOW_DISPLAY" -eq 0 ]; then
    QEMU+=(-display none)
else
    echo "Display enabled."
fi

if [ "$DEBUG" -eq 1 ]; then
    QEMU+=(-s -S)
    echo "GDB stub listening on localhost:1234"
    echo "Connect with: gdb -ex 'target remote :1234' kernel/target/x86_64-unknown-none/debug/vahi_kernel"
fi

echo "QEMU: ${QEMU[*]}"
exec "${QEMU[@]}"
