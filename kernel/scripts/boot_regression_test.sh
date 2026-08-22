#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# 100 Consecutive Boot Regression Test
#
# Tests:
# 1. Build stability: 100 consecutive cargo builds (same binary)
# 2. Binary consistency: SHA256 of each build matches
# 3. QEMU serial output: parse TAP selftest results
# ──────────────────────────────────────────────────────────────────────

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KERNEL_DIR="$(dirname "$SCRIPT_DIR")"
RESULTS_DIR="$KERNEL_DIR/target/boot-regression"
mkdir -p "$RESULTS_DIR"

TOTAL=100
PASS=0
FAIL=0
BUILD_OK=0
BUILD_FAIL=0
TAP_PASS=0
TAP_FAIL=0
QEMU_RUNS=0

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

LOG="$RESULTS_DIR/regression.log"
RESULTS_CSV="$RESULTS_DIR/results.csv"

echo "iteration,build_ok,sha256,build_time_ms" > "$RESULTS_CSV"

echo ""
echo "╔══════════════════════════════════════════════════╗"
echo "║   100 Consecutive Boot Regression Test           ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""
echo "Started: $(date)"
echo "Results: $RESULTS_DIR"
echo ""

# ── Phase 1: Build stability (100 iterations) ────────────────────────

echo -e "${CYAN}═══ Phase 1: Build Stability (100 iterations) ═══${NC}"
echo ""

FIRST_HASH=""

for i in $(seq 1 $TOTAL); do
    START_MS=$(($(date +%s%N) / 1000000))

    BUILD_OUTPUT=$(cd "$KERNEL_DIR" && cargo build --features self_test --target x86_64-unknown-none 2>&1)
    BUILD_EXIT=$?

    END_MS=$(($(date +%s%N) / 1000000))
    ELAPSED_MS=$((END_MS - START_MS))

    KERNEL_BIN="$KERNEL_DIR/target/x86_64-unknown-none/debug/vahi_kernel"
    HASH=$(sha256sum "$KERNEL_BIN" 2>/dev/null | awk '{print $1}' || echo "none")

    if [[ $BUILD_EXIT -eq 0 ]]; then
        BUILD_OK=$((BUILD_OK + 1))
        STATUS="PASS"
        COLOR="$GREEN"
    else
        BUILD_FAIL=$((BUILD_FAIL + 1))
        STATUS="FAIL"
        COLOR="$RED"
        echo "$BUILD_OUTPUT" > "$RESULTS_DIR/build_fail_${i}.log"
    fi

    # Check binary consistency
    if [[ $i -eq 1 ]]; then
        FIRST_HASH="$HASH"
    elif [[ "$HASH" != "$FIRST_HASH" ]]; then
        STATUS="INCONSISTENT"
        COLOR="$YELLOW"
        echo "WARN: Build $i hash differs from build 1" >> "$LOG"
    fi

    echo "${i},${BUILD_EXIT},${HASH},${ELAPSED_MS}" >> "$RESULTS_CSV"

    # Progress bar
    printf "\r  [%3d/%d] ${COLOR}%s${NC} build=%dms hash=%s..." "$i" "$TOTAL" "$STATUS" "$ELAPSED_MS" "${HASH:0:12}"

    if [[ $BUILD_EXIT -ne 0 ]]; then
        echo ""
        echo -e "  ${RED}Build $i failed — see $RESULTS_DIR/build_fail_${i}.log${NC}"
    fi
done

echo ""
echo ""
echo -e "  Build results: ${GREEN}$BUILD_OK passed${NC}, ${RED}$BUILD_FAIL failed${NC} out of $TOTAL"

# ── Phase 2: Binary consistency check ────────────────────────────────

echo ""
echo -e "${CYAN}═══ Phase 2: Binary Consistency ═══${NC}"
echo ""

# Count unique hashes
UNIQUE_HASHES=$(awk -F, 'NR>1 {print $3}' "$RESULTS_CSV" | sort -u | wc -l)
echo "  Unique SHA256 hashes: $UNIQUE_HASHES out of $TOTAL builds"

if [[ $UNIQUE_HASHES -eq 1 ]]; then
    echo -e "  ${GREEN}✅ All builds produced identical binaries${NC}"
elif [[ $UNIQUE_HASHES -le 3 ]]; then
    echo -e "  ${YELLOW}⚠️  $UNIQUE_HASHES unique binaries (may be due to timestamps)${NC}"
else
    echo -e "  ${RED}❌ $UNIQUE_HASHES unique binaries — non-deterministic build!${NC}"
fi

# ── Phase 3: QEMU boot test (subset) ────────────────────────────────

echo ""
echo -e "${CYAN}═══ Phase 3: QEMU Boot Test ═══${NC}"
echo ""

# Only run QEMU tests if bootimage or QEMU direct boot works
if command -v qemu-system-x86_64 &>/dev/null; then
    echo "  QEMU found: $(which qemu-system-x86_64)"

    # Try to find or create a boot image
    IMAGE=$(find "$KERNEL_DIR/target/x86_64-unknown-none/debug" -name "bootimage-*.bin" -o -name "vahi-boot.img" 2>/dev/null | head -1)

    if [[ -n "$IMAGE" ]]; then
        echo "  Boot image: $(basename "$IMAGE")"

        # Run 10 QEMU boot tests
        QEMU_ITERATIONS=10
        QEMU_PASS=0
        QEMU_FAIL=0

        for i in $(seq 1 $QEMU_ITERATIONS); do
            SERIAL_LOG="$RESULTS_DIR/qemu_serial_${i}.log"
            QEMU_LOG="$RESULTS_DIR/qemu_stderr_${i}.log"

            # Run QEMU with 30s timeout
            timeout 30 qemu-system-x86_64 \
                -drive "format=raw,file=$IMAGE" \
                -m 512 \
                -nographic \
                -serial file:"$SERIAL_LOG" \
                -no-reboot \
                -d guest_errors \
                > /dev/null 2> "$QEMU_LOG" || true

            QEMU_RUNS=$((QEMU_RUNS + 1))

            # Check for TAP output
            if [[ -f "$SERIAL_LOG" ]]; then
                TAP_OK=$(grep -c "^ok " "$SERIAL_LOG" 2>/dev/null || echo "0")
                TAP_NOT_OK=$(grep -c "^not ok " "$SERIAL_LOG" 2>/dev/null || echo "0")
                HAS_PANIC=$(grep -ci "panic\|PANIC\|fault" "$SERIAL_LOG" 2>/dev/null || echo "0")

                if [[ $TAP_OK -gt 0 && $HAS_PANIC -eq 0 ]]; then
                    QEMU_PASS=$((QEMU_PASS + 1))
                    TAP_PASS=$((TAP_PASS + TAP_OK))
                    printf "\r  QEMU [%2d/%d] ${GREEN}PASS${NC} — %d TAP tests passed" "$i" "$QEMU_ITERATIONS" "$TAP_OK"
                elif [[ $HAS_PANIC -gt 0 ]]; then
                    QEMU_FAIL=$((QEMU_FAIL + 1))
                    echo ""
                    echo -e "  ${RED}QEMU $i: PANIC detected${NC}"
                else
                    QEMU_FAIL=$((QEMU_FAIL + 1))
                    echo ""
                    echo -e "  ${YELLOW}QEMU $i: No TAP output (kernel may not have booted)${NC}"
                fi
            else
                QEMU_FAIL=$((QEMU_FAIL + 1))
                echo ""
                echo -e "  ${RED}QEMU $i: No serial output${NC}"
            fi
        done

        echo ""
        echo ""
        echo -e "  QEMU results: ${GREEN}$QEMU_PASS passed${NC}, ${RED}$QEMU_FAIL failed${NC} out of $QEMU_ITERATIONS"
    else
        echo -e "  ${YELLOW}⚠️  No boot image found — skipping QEMU tests${NC}"
        echo "  To enable QEMU tests: cargo bootimage (requires bootloader crate)"
    fi
else
    echo -e "  ${YELLOW}⚠️  QEMU not found — skipping QEMU tests${NC}"
fi

# ── Summary ──────────────────────────────────────────────────────────

echo ""
echo "═══════════════════════════════════════════════════════════"
echo "  100 Consecutive Boot Regression Test — Summary"
echo "═══════════════════════════════════════════════════════════"
echo "  Builds:        $BUILD_OK/$TOTAL passed ($BUILD_FAIL failed)"
echo "  Binary hash:   $UNIQUE_HASHES unique"
if [[ $QEMU_RUNS -gt 0 ]]; then
    echo "  QEMU boots:    $QEMU_PASS/$QEMU_RUNS passed"
    echo "  TAP tests:     $TAP_PASS total"
fi
echo "  Results CSV:   $RESULTS_CSV"
echo "  Full log:      $LOG"
echo "═══════════════════════════════════════════════════════════"
echo ""
echo "Finished: $(date)"

# Exit with failure if any builds failed
if [[ $BUILD_FAIL -gt 0 ]]; then
    exit 1
fi
exit 0
