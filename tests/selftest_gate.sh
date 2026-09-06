#!/usr/bin/env bash
# tests/selftest_gate.sh — Bounded selftest gate for CI and local verification.
#
# Builds nothing; run after `cargo build --features self_test` + bootimage step.
# Boots the kernel in QEMU, waits for the TAP summary line on serial, exits:
#   0 — `# N/N passed, 0 failed` observed within TIMEOUT
#   1 — TAP summary missing, or any failure/panic markers in serial output
#   2 — kernel built without self_test, or bootimage missing
#
# Usage: bash tests/selftest_gate.sh [timeout_seconds] [smp]

set -uo pipefail

TIMEOUT=${1:-180}
SMP=${2:-1}
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="${VAHI_IMAGE:-$ROOT_DIR/bootimage-vahi_kernel.bin}"
OVMF="$ROOT_DIR/OVMF.fd"
LOG="${VAHI_GATE_LOG:-$ROOT_DIR/tests/gate_last_run.log}"

if [ ! -f "$IMAGE" ]; then
    echo "GATE ERROR: bootimage not found at $IMAGE"
    echo "Build it: cd kernel && cargo build --features self_test -Zbuild-std=core,alloc --target x86_64-unknown-none"
    echo "          py builder/build_limine_image.py --kernel target/x86_64-unknown-none/debug/vahi_kernel --output bootimage-vahi_kernel.bin"
    exit 2
fi

rm -f "$LOG"
QEMU_LOG=$(mktemp)
trap 'rm -f "$QEMU_LOG"' EXIT

# Convert to Windows-style paths for QEMU on Git Bash
WIN_LOG=$(cygpath -w "$LOG" 2>/dev/null || echo "$LOG")
WIN_OVMF=$(cygpath -w "$OVMF" 2>/dev/null || echo "$OVMF")
WIN_IMAGE=$(cygpath -w "$IMAGE" 2>/dev/null || echo "$IMAGE")

echo "=== Selftest Gate ==="
echo "Image: $IMAGE | SMP: $SMP | Timeout: ${TIMEOUT}s"

# Timeout 124 = kernel kept running past the window; the gate reads what serial
# captured regardless, since TAP output lands early in the boot.
# Use a subshell to ensure QEMU is killed on timeout
timeout "$TIMEOUT" bash -c "
    qemu-system-x86_64 \
        -drive \"if=pflash,format=raw,file=$WIN_OVMF\" \
        -drive \"format=raw,file=$WIN_IMAGE\" \
        -m 512 -smp \"$SMP\" \
        -serial \"file:$WIN_LOG\" \
        -display none -no-reboot -accel tcg \
        >/dev/null 2>&1
" || true
QEMU_RC=$?

# Ensure QEMU is dead (timeout may leave it running on some platforms)
pkill -f "qemu-system-x86_64.*$WIN_IMAGE" 2>/dev/null || true

if [ ! -f "$LOG" ] || [ ! -s "$LOG" ]; then
    echo "GATE FAIL: no serial output captured (qemu rc=$QEMU_RC)"
    exit 1
fi

# Strip ANSI escapes before matching: Limine paints its boot lines with
# cursor-position sequences that would otherwise corrupt the TAP match.
CLEAN=$(tr -d '\033' < "$LOG" | sed 's/\[[0-9][0-9;]*[A-Za-z]//g')

echo "$CLEAN" | grep -E "KERNEL PANIC|Panicked at|Bail out!" && {
    echo "GATE FAIL: panic marker in serial output"
    exit 1
}

if echo "$CLEAN" | grep -q "^not ok "; then
    echo "$CLEAN" | grep "^not ok " | head -20
    echo "GATE FAIL: failing TAP tests above"
    exit 1
fi

SUMMARY=$(echo "$CLEAN" | grep -oE "^# [0-9]+/[0-9]+ passed, [0-9]+ failed" | tail -1)
if [ -z "$SUMMARY" ]; then
    echo "GATE FAIL: TAP summary line not found in serial output (qemu rc=$QEMU_RC)"
    echo "Last serial lines:"
    tail -5 "$LOG" | tr -d '\033'
    exit 1
fi

TOTAL=$(echo "$SUMMARY" | sed -E 's|# ([0-9]+)/[0-9]+ passed, [0-9]+ failed|\1|')
PASSED=$(echo "$SUMMARY" | sed -E 's|# [0-9]+/([0-9]+) passed, [0-9]+ failed|\1|')
FAILED=$(echo "$SUMMARY" | sed -E 's|# [0-9]+/[0-9]+ passed, ([0-9]+) failed|\1|')
if [ "$FAILED" -ne 0 ] || [ "$PASSED" -ne "$TOTAL" ] || [ "$TOTAL" -eq 0 ]; then
    echo "GATE FAIL: $SUMMARY"
    exit 1
fi

echo "GATE PASS: $SUMMARY"
exit 0
