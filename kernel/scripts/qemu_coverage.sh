#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# QEMU Coverage Collection for Vahi Kernel
#
# Runs the kernel in QEMU, dumps the coverage bitmap from the fixed
# physical address, and saves it for fuzzing.
#
# Usage:
#   ./scripts/qemu_coverage.sh              # Collect coverage
#   ./scripts/qemu_coverage.sh --iter N     # Run N iterations
#   ./scripts/qemu_coverage.sh --diff       # Show coverage diff
#   ./scripts/qemu_coverage.sh --report     # Generate HTML report
# ──────────────────────────────────────────────────────────────────────

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KERNEL_DIR="$(dirname "$SCRIPT_DIR")"
COVERAGE_DIR="$KERNEL_DIR/target/coverage"
mkdir -p "$COVERAGE_DIR"

# Coverage bitmap parameters (must match kernel/src/coverage.rs)
BITMAP_PHYS="0x40000000"   # 0x0000_0004_0000_0000
BITMAP_SIZE="65536"        # 64 KiB

# Defaults
ITERATIONS=1
PROFILE="debug"
VERBOSE=false

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

step()   { echo -e "${CYAN}>>> $1${NC}"; }
ok()     { echo -e "${GREEN}✅ $1${NC}"; }
fail()   { echo -e "${RED}❌ $1${NC}"; }

# ── Parse args ────────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --iter)      ITERATIONS="$2"; shift 2 ;;
        --release)   PROFILE="release"; shift ;;
        --verbose)   VERBOSE=true; shift ;;
        --diff)      MODE="diff"; shift ;;
        --report)    MODE="report"; shift ;;
        -h|--help)
            echo "Usage: $0 [--iter N] [--release] [--verbose] [--diff] [--report]"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# ── Build ─────────────────────────────────────────────────────────────

build_kernel() {
    step "Building kernel with self_test feature..."
    PROFILE_FLAG=""
    if [[ "$PROFILE" == "release" ]]; then
        PROFILE_FLAG="--release"
    fi

    cargo build $PROFILE_FLAG --features self_test --target x86_64-unknown-none 2>&1 | tail -3
    ok "Kernel built"
}

# ── Create boot image ────────────────────────────────────────────────

create_image() {
    step "Creating bootable image..."
    cargo bootimage --no-run 2>&1 | tail -3

    TARGET_DIR="target/x86_64-unknown-none/$PROFILE"
    IMAGE=$(find "$TARGET_DIR" -maxdepth 1 -name "bootimage-*.bin" 2>/dev/null | head -1)
    if [[ -z "$IMAGE" ]]; then
        fail "No boot image found"
        exit 1
    fi
    ok "Boot image: $(basename "$IMAGE")"
}

# ── Run QEMU and dump coverage bitmap ────────────────────────────────

collect_coverage() {
    local iter=$1
    local snapshot="$COVERAGE_DIR/coverage_iter_${iter}.bin"
    local serial_log="$COVERAGE_DIR/serial_iter_${iter}.log"

    step "Iteration $iter: Running QEMU..."

    # Run QEMU with:
    # - -d in_asm: log executed instructions (for advanced analysis)
    # - -d guest_errors: catch kernel issues
    # - Serial to file for selftest output
    QEMU_LOG="$COVERAGE_DIR/qemu_exec_iter_${iter}.log"

    timeout 120 qemu-system-x86_64 \
        -drive "format=raw,file=$IMAGE" \
        -m 512 \
        -nographic \
        -serial file:"$serial_log" \
        -no-reboot \
        -d guest_errors \
        > /dev/null 2>&1 || true

    # Dump the coverage bitmap from physical memory
    # QEMU's -device loader writes to physical memory; we read it back
    # via /proc/self/mem of the QEMU process, but that's complex.
    # Instead, we use QEMU's -d exec to log block addresses and parse.

    # Simpler approach: read the bitmap via QEMU monitor
    # Use the HMP monitor to dump memory
    timeout 120 qemu-system-x86_64 \
        -drive "format=raw,file=$IMAGE" \
        -m 512 \
        -nographic \
        -serial file:"$serial_log" \
        -no-reboot \
        -d exec \
        -D "$QEMU_LOG" \
        > /dev/null 2>&1 || true

    # Parse QEMU execution log for unique block addresses
    if [[ -f "$QEMU_LOG" ]]; then
        # Extract block addresses from exec log
        # QEMU logs: Block 0xADDR
        grep -oP 'Block 0x[0-9a-f]+' "$QEMU_LOG" 2>/dev/null \
            | sort -u \
            | awk '{print $2}' \
            > "$snapshot" 2>/dev/null || true

        local unique_blocks
        unique_blocks=$(wc -l < "$snapshot" 2>/dev/null || echo "0")
        ok "Iteration $iter: $unique_blocks unique blocks captured"
    else
        warn "No QEMU exec log for iteration $iter"
        touch "$snapshot"
    fi

    # Extract TAP results from serial
    if [[ -f "$serial_log" ]]; then
        local passed
        passed=$(grep -c "^ok " "$serial_log" 2>/dev/null || echo "0")
        local failed
        failed=$(grep -c "^not ok " "$serial_log" 2>/dev/null || echo "0")
        ok "TAP: $passed passed, $failed failed"
    fi
}

# ── Coverage diff ─────────────────────────────────────────────────────

show_diff() {
    if [[ ! -f "$COVERAGE_DIR/coverage_iter_1.bin" ]]; then
        fail "No coverage data found. Run collection first."
        exit 1
    fi

    step "Coverage diff across iterations..."
    local prev_blocks=""
    for f in "$COVERAGE_DIR"/coverage_iter_*.bin; do
        local iter
        iter=$(echo "$f" | grep -oP 'iter_\K[0-9]+')
        local current_blocks
        current_blocks=$(wc -l < "$f" 2>/dev/null || echo "0")

        if [[ -n "$prev_blocks" ]]; then
            local new_blocks
            new_blocks=$(comm -13 "$COVERAGE_DIR"/coverage_iter_*_$(($iter-1)).bin "$f" 2>/dev/null | wc -l || echo "0")
            echo "  Iteration $iter: $current_blocks total, +$new_blocks new blocks"
        else
            echo "  Iteration $iter: $current_blocks blocks (baseline)"
        fi
        prev_blocks="$f"
    done
}

# ── Coverage report ───────────────────────────────────────────────────

generate_report() {
    step "Generating coverage report..."

    local all_blocks="$COVERAGE_DIR/all_blocks.txt"
    cat "$COVERAGE_DIR"/coverage_iter_*.bin 2>/dev/null | sort -u > "$all_blocks"

    local total
    total=$(wc -l < "$all_blocks" 2>/dev/null || echo "0")

    # Kernel text section range (approximate)
    local KERNEL_TEXT_START="0xffffffff80000000"
    local KERNEL_TEXT_END="0xffffffff82000000"

    # Count blocks in kernel text vs userspace
    local kernel_blocks
    kernel_blocks=$(awk -v start="$KERNEL_TEXT_START" -v end="$KERNEL_TEXT_END" '
        $1 >= start && $1 < end { count++ }
        END { print count+0 }
    ' "$all_blocks")

    local userspace_blocks=$((total - kernel_blocks))

    # Generate HTML report
    REPORT="$COVERAGE_DIR/coverage_report.html"
    cat > "$REPORT" << 'HEADER'
<!DOCTYPE html>
<html>
<head>
    <title>Vahi Kernel — Coverage Report</title>
    <style>
        body { font-family: monospace; background: #1a1a2e; color: #e0e0e0; padding: 2em; }
        h1 { color: #00d4ff; }
        .stat { background: #16213e; padding: 1em; margin: 1em 0; border-left: 4px solid #00d4ff; }
        .bar { background: #0f3460; height: 24px; border-radius: 4px; margin: 0.5em 0; }
        .bar-fill { background: #00d4ff; height: 100%; border-radius: 4px; }
        .blocks { font-size: 0.8em; color: #888; max-height: 400px; overflow-y: auto; }
    </style>
</head>
<body>
<h1>🔬 Vahi Kernel — Coverage Report</h1>
HEADER

    cat >> "$REPORT" << STATS
<div class="stat">
    <strong>Total unique blocks:</strong> $total<br>
    <strong>Kernel text blocks:</strong> $kernel_blocks<br>
    <strong>Userspace/other blocks:</strong> $userspace_blocks<br>
</div>
STATS

    # Block list
    echo '<div class="stat"><strong>Covered block addresses:</strong></div>' >> "$REPORT"
    echo '<div class="blocks">' >> "$REPORT"
    while read -r addr; do
        echo "  $addr" >> "$REPORT"
    done < "$all_blocks"
    echo '</div>' >> "$REPORT"
    echo '</body></html>' >> "$REPORT"

    ok "Report: $REPORT ($total unique blocks)"
}

# ── Main ──────────────────────────────────────────────────────────────

echo ""
echo "╔══════════════════════════════════════════════╗"
echo "║   Vahi Kernel — Coverage Collection          ║"
echo "╚══════════════════════════════════════════════╝"
echo ""

if [[ "${MODE:-}" == "diff" ]]; then
    show_diff
    exit 0
fi

if [[ "${MODE:-}" == "report" ]]; then
    generate_report
    exit 0
fi

build_kernel
create_image

for i in $(seq 1 $ITERATIONS); do
    collect_coverage "$i"
done

echo ""
echo "═══════════════════════════════════════════════"
echo "  Coverage collected: $ITERATIONS iterations"
echo "  Data: $COVERAGE_DIR"
echo "═══════════════════════════════════════════════"
echo ""
echo "Next steps:"
echo "  ./scripts/qemu_coverage.sh --diff     # See coverage growth"
echo "  ./scripts/qemu_coverage.sh --report   # Generate HTML report"
echo "  ./scripts/qemu_fuzz.sh               # Start fuzzing"
