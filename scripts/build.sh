#!/bin/bash
# build.sh — Build the Vahi kernel and create a bootable disk image.
#
# Usage:
#   ./scripts/build.sh              # Release build + disk image
#   ./scripts/build.sh --debug      # Debug build + disk image
#   ./scripts/build.sh --kernel     # Build kernel only (no image)
#   ./scripts/build.sh --image      # Create disk image only (kernel must be built)
#
# Output: bootimage-vahi_kernel.bin (or .img)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
KERNEL_DIR="$ROOT_DIR/kernel"
BUILDER_DIR="$ROOT_DIR/builder"
OUTPUT="$ROOT_DIR/bootimage-vahi_kernel.bin"

PROFILE="release"
BUILD_KERNEL=1
BUILD_IMAGE=1

for arg in "$@"; do
    case "$arg" in
        --debug)   PROFILE="debug" ;;
        --kernel)  BUILD_IMAGE=0 ;;
        --image)   BUILD_KERNEL=0 ;;
        -h|--help)
            echo "Usage: $0 [--debug] [--kernel] [--image]"
            echo "  --debug   Build in debug mode (default: release)"
            echo "  --kernel  Build kernel only, skip disk image"
            echo "  --image   Create disk image only, skip kernel build"
            exit 0
            ;;
    esac
done

# ── Step 1: Build kernel ──────────────────────────────────────────────
if [ "$BUILD_KERNEL" -eq 1 ]; then
    echo "=== Building kernel ($PROFILE) ==="
    cd "$KERNEL_DIR"
    if [ "$PROFILE" = "release" ]; then
        cargo build --release --target x86_64-unknown-none
    else
        cargo build --target x86_64-unknown-none
    fi
    echo "Kernel built: kernel/target/x86_64-unknown-none/$PROFILE/vahi_kernel"
fi

# ── Step 2: Create bootable disk image ────────────────────────────────
if [ "$BUILD_IMAGE" -eq 1 ]; then
    echo ""
    echo "=== Creating bootable disk image ==="
    cd "$ROOT_DIR"

    # The Python builder handles GPT + FAT32 + Limine setup.
    python3 "$BUILDER_DIR/build_limine_image.py" \
        --kernel "$KERNEL_DIR/target/x86_64-unknown-none/$PROFILE/vahi_kernel" \
        --output "$OUTPUT" \
        ${INITRD:+--initrd "$INITRD"} \
        2>&1 || {
            echo ""
            echo "ERROR: Image creation failed."
            echo "Ensure Limine binaries are available. See builder/README.md."
            exit 1
        }
fi

echo ""
echo "=== Build complete ==="
echo "Image: $OUTPUT"
echo ""
echo "Run with:"
echo "  qemu-system-x86_64 -drive file=$OUTPUT -m 512M -serial stdio"
