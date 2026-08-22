#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# Vahi Kernel — Limine Boot Image Builder
#
# Downloads pre-built Limine binaries and creates a FAT12 bootable
# floppy image for QEMU testing.
#
# Usage:
#   ./scripts/build_limine_image.sh              # Create 1.44MB floppy
#   ./scripts/build_limine_image.sh --disk       # Create 64MB disk
#   ./scripts/build_limine_image.sh --iso        # Create hybrid ISO
#
# Prerequisites:
#   - Limine binaries (auto-downloaded if missing)
#   - mtools (for FAT image creation)
#   - xorriso (for ISO creation, --iso mode only)
# ──────────────────────────────────────────────────────────────────────

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KERNEL_DIR="$(dirname "$SCRIPT_DIR")"
LIMINE_DIR="$KERNEL_DIR/limine"
LIMINE_VERSION="v8.0.4-binary"
LIMINE_URL="https://github.com/limine-bootloader/limine/releases/download/$LIMINE_VERSION/$LIMINE_VERSION.tar.gz"

# ── Defaults ──────────────────────────────────────────────────────────

MODE="floppy"
OUTPUT=""

# ── Parse args ────────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --disk)  MODE="disk"; shift ;;
        --iso)   MODE="iso"; shift ;;
        --output) OUTPUT="$2"; shift 2 ;;
        -h|--help)
            echo "Usage: $0 [--disk] [--iso] [--output FILE]"
            echo "  --disk   Create 64MB hard disk image"
            echo "  --iso    Create hybrid ISO (requires xorriso)"
            echo "  Default: Create 1.44MB floppy image"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# ── Colors ────────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

step()   { echo -e "${CYAN}>>> $1${NC}"; }
ok()     { echo -e "${GREEN}✅ $1${NC}"; }
fail()   { echo -e "${RED}❌ $1${NC}"; }
warn()   { echo -e "${YELLOW}⚠️  $1${NC}"; }

# ── Download Limine binaries ──────────────────────────────────────────

download_limine() {
    if [[ -f "$LIMINE_DIR/limine-bios.sys" ]]; then
        ok "Limine binaries already present"
        return 0
    fi

    step "Downloading Limine $LIMINE_VERSION..."
    mkdir -p "$LIMINE_DIR"

    if command -v curl &>/dev/null; then
        curl -sL "$LIMINE_URL" | tar xz --strip-components=1 -C "$LIMINE_DIR"
    elif command -v wget &>/dev/null; then
        wget -qO- "$LIMINE_URL" | tar xz --strip-components=1 -C "$LIMINE_DIR"
    else
        fail "Neither curl nor wget found"
        exit 1
    fi

    if [[ ! -f "$LIMINE_DIR/limine-bios.sys" ]]; then
        fail "Limine download failed"
        exit 1
    fi

    ok "Limine binaries downloaded"
}

# ── Build kernel ─────────────────────────────────────────────────────

build_kernel() {
    step "Building kernel..."
    cd "$KERNEL_DIR"

    cargo build --features self_test --target x86_64-unknown-none 2>&1 | tail -3

    local kernel_path="target/x86_64-unknown-none/debug/vahi_kernel"
    if [[ ! -f "$kernel_path" ]]; then
        fail "Kernel build failed — no binary at $kernel_path"
        exit 1
    fi

    ok "Kernel built: $(ls -lh "$kernel_path" | awk '{print $5}')"
}

# ── Create FAT12 floppy image ────────────────────────────────────────

create_floppy() {
    local output="${OUTPUT:-$KERNEL_DIR/target/vahi_boot.img}"

    step "Creating 1.44MB FAT12 floppy image..."

    if ! command -v mkfs.fat &>/dev/null && ! command -v mformat &>/dev/null; then
        fail "mtools/mkfs.fat not found. Install mtools."
        echo "  Ubuntu/Debian: sudo apt install mtools"
        echo "  macOS: brew install mtools"
        exit 1
    fi

    # Create blank 1.44MB image
    dd if=/dev/zero of="$output" bs=512 count=2880 2>/dev/null

    # Format as FAT12
    mkfs.fat -F 12 -n "VAHI" "$output" 2>/dev/null

    # Copy files to image using mtools
    mmd -i "$output" ::/boot
    mcopy -i "$output" "$KERNEL_DIR/target/x86_64-unknown-none/debug/vahi_kernel" ::/boot/vahi_kernel

    if [[ -f "$KERNEL_DIR/initrd.tar" ]]; then
        mcopy -i "$output" "$KERNEL_DIR/initrd.tar" ::/boot/initrd.tar
    fi

    mcopy -i "$output" "$KERNEL_DIR/limine.conf" ::/limine.conf

    # Install Limine BIOS bootloader
    if [[ -f "$LIMINE_DIR/limine bios-install" ]]; then
        "$LIMINE_DIR/limine" bios-install "$output" 2>/dev/null
    elif [[ -f "$LIMINE_DIR/limine.exe" ]]; then
        "$LIMINE_DIR/limine.exe" bios-install "$output" 2>/dev/null
    else
        warn "limine bios-install not found — image may not be bootable"
    fi

    ok "Floppy image: $output ($(du -h "$output" | cut -f1))"
    echo "$output"
}

# ── Create hard disk image ───────────────────────────────────────────

create_disk() {
    local output="${OUTPUT:-$KERNEL_DIR/target/vahi_disk.img}"

    step "Creating 64MB hard disk image..."

    # Create blank 64MB image
    dd if=/dev/zero of="$output" bs=1M count=64 2>/dev/null

    # Create MBR partition table with single FAT32 partition
    # Using sfdisk for non-interactive partitioning
    echo -e ',,0c,*' | sfdisk "$output" 2>/dev/null || {
        warn "sfdisk not available, using raw image (no partitions)"
    }

    # Format as FAT32 (or FAT12 if small enough)
    mkfs.fat -F 32 -n "VAHI" "$output" 2>/dev/null || {
        mkfs.fat -F 12 -n "VAHI" "$output" 2>/dev/null
    }

    # Copy files
    mmd -i "$output" ::/boot
    mcopy -i "$output" "$KERNEL_DIR/target/x86_64-unknown-none/debug/vahi_kernel" ::/boot/vahi_kernel

    if [[ -f "$KERNEL_DIR/initrd.tar" ]]; then
        mcopy -i "$output" "$KERNEL_DIR/initrd.tar" ::/boot/initrd.tar
    fi

    mcopy -i "$output" "$KERNEL_DIR/limine.conf" ::/limine.conf

    # Install Limine
    "$LIMINE_DIR/limine" bios-install "$output" 2>/dev/null || true

    ok "Disk image: $output ($(du -h "$output" | cut -f1))"
    echo "$output"
}

# ── Create hybrid ISO ────────────────────────────────────────────────

create_iso() {
    local output="${OUTPUT:-$KERNEL_DIR/target/vahi.iso}"
    local iso_root="$KERNEL_DIR/target/iso_root"

    step "Creating hybrid ISO..."

    if ! command -v xorriso &>/dev/null; then
        fail "xorriso not found"
        echo "  Ubuntu/Debian: sudo apt install xorriso"
        exit 1
    fi

    # Prepare ISO root
    rm -rf "$iso_root"
    mkdir -p "$iso_root/boot"
    mkdir -p "$iso_root/EFI/BOOT"

    cp "$KERNEL_DIR/target/x86_64-unknown-none/debug/vahi_kernel" "$iso_root/boot/"
    [[ -f "$KERNEL_DIR/initrd.tar" ]] && cp "$KERNEL_DIR/initrd.tar" "$iso_root/boot/"
    cp "$KERNEL_DIR/limine.conf" "$iso_root/"

    # Copy Limine binaries for ISO
    cp "$LIMINE_DIR/limine-bios-cd.bin" "$iso_root/"
    cp "$LIMINE_DIR/limine-uefi-cd.bin" "$iso_root/"
    cp "$LIMINE_DIR/BOOTX64.EFI" "$iso_root/EFI/BOOT/"
    cp "$LIMINE_DIR/BOOTIA32.EFI" "$iso_root/EFI/BOOT/" 2>/dev/null || true

    # Create ISO
    xorriso -as mkisofs -R -r -J \
        -b limine-bios-cd.bin \
        -no-emul-boot -boot-load-size 4 -boot-info-table \
        --efi-boot limine-uefi-cd.bin \
        --efi-boot-part --efi-boot-image \
        --protective-msdos-label \
        "$iso_root" -o "$output" 2>/dev/null

    # Install Limine BIOS
    "$LIMINE_DIR/limine" bios-install "$output" 2>/dev/null || true

    rm -rf "$iso_root"

    ok "ISO image: $output ($(du -h "$output" | cut -f1))"
    echo "$output"
}

# ── Main ──────────────────────────────────────────────────────────────

echo ""
echo "╔══════════════════════════════════════════════╗"
echo "║   Vahi Kernel — Limine Image Builder        ║"
echo "╚══════════════════════════════════════════════╝"
echo ""

download_limine
build_kernel

case "$MODE" in
    floppy) create_floppy ;;
    disk)   create_disk ;;
    iso)    create_iso ;;
esac

echo ""
ok "Done! Boot with:"
echo "  qemu-system-x86_64 -drive format=raw,file=<image> -m 512 -nographic"
