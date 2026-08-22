#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# Vahi Kernel — QEMU Limine Test Runner
#
# Builds kernel, creates Limine boot image, launches QEMU,
# captures serial output, and parses TAP results.
#
# Usage:
#   ./scripts/qemu_limine_test.sh                    # Default (debug, 1 CPU)
#   ./scripts/qemu_limine_test.sh --release          # Release build
#   ./scripts/qemu_limine_test.sh --smp 4            # 4-CPU SMP
#   ./scripts/qemu_limine_test.sh --kvm              # Use KVM acceleration
#   ./scripts/qemu_limine_test.sh --build-image      # Rebuild Limine image
# ──────────────────────────────────────────────────────────────────────

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KERNEL_DIR="$(dirname "$SCRIPT_DIR")"
TEST_DIR="$KERNEL_DIR/target/qemu_tests"
mkdir -p "$TEST_DIR"

# ── Defaults ──────────────────────────────────────────────────────────

RELEASE=false
SMP=1
TIMEOUT=120
KVM=false
BUILD_IMAGE=false
BOOT_IMAGE=""

# ── Parse args ────────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --release)      RELEASE=true; shift ;;
        --smp)          SMP="$2"; shift 2 ;;
        --timeout)      TIMEOUT="$2"; shift 2 ;;
        --kvm)          KVM=true; shift ;;
        --build-image)  BUILD_IMAGE=true; shift ;;
        --image)        BOOT_IMAGE="$2"; shift 2 ;;
        -h|--help)
            echo "Usage: $0 [--release] [--smp N] [--timeout S] [--kvm] [--build-image] [--image FILE]"
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

# ── Check prerequisites ──────────────────────────────────────────────

check_prereqs() {
    if ! command -v qemu-system-x86_64 &>/dev/null; then
        fail "qemu-system-x86_64 not found"
        echo "Install: https://www.qemu.org/download/"
        exit 1
    fi
    ok "QEMU found: $(which qemu-system-x86_64)"
}

# ── Build kernel and create image ────────────────────────────────────

build_and_image() {
    step "Building kernel (self_test + Limine)..."
    PROFILE_FLAG=""
    if $RELEASE; then
        PROFILE_FLAG="--release"
    fi

    cargo build $PROFILE_FLAG --features self_test --target x86_64-unknown-none 2>&1 | tail -5
    ok "Kernel built"

    if [[ -z "$BOOT_IMAGE" ]] || $BUILD_IMAGE; then
        step "Creating Limine boot image..."
        bash "$SCRIPT_DIR/build_limine_image.sh" 2>&1 | tail -3
        BOOT_IMAGE="$KERNEL_DIR/target/vahi_boot.img"
    fi

    if [[ ! -f "$BOOT_IMAGE" ]]; then
        fail "Boot image not found: $BOOT_IMAGE"
        exit 1
    fi
    ok "Boot image: $BOOT_IMAGE ($(du -h "$BOOT_IMAGE" | cut -f1))"
}

# ── Run QEMU ─────────────────────────────────────────────────────────

run_qemu() {
    step "Launching QEMU (timeout: ${TIMEOUT}s, SMP: $SMP)..."

    QEMU_ARGS=(
        -drive "format=raw,file=$BOOT_IMAGE"
        -m 512
        -nographic
        -serial stdio
        -no-reboot
        -d guest_errors
        -smp "$SMP"
    )

    if $KVM && [[ "$(uname)" != "Darwin" ]]; then
        QEMU_ARGS+=(-accel kvm)
    fi

    SERIAL_LOG="$TEST_DIR/serial.log"
    QEMU_STDERR="$TEST_DIR/qemu_stderr.log"

    EXIT_CODE=0
    timeout "$TIMEOUT" qemu-system-x86_64 "${QEMU_ARGS[@]}" \
        >"$SERIAL_LOG" 2>"$QEMU_STDERR" || EXIT_CODE=$?

    if [[ $EXIT_CODE -eq 124 ]]; then
        warn "QEMU timed out after ${TIMEOUT}s"
    fi

    SERIAL_OUTPUT=$(cat "$SERIAL_LOG" 2>/dev/null || echo "")

    echo ""
    echo -e "${CYAN}─── Serial Output ───${NC}"
    echo "$SERIAL_OUTPUT" | tail -80
    echo -e "${CYAN}─── End Serial ──────${NC}"
    echo ""
}

# ── Parse TAP ─────────────────────────────────────────────────────────

parse_tap() {
    local output="$1"
    local passed=0
    local failed=0
    local bail_out=false
    local pass_names=()
    local fail_names=()

    while IFS= read -r line; do
        line=$(echo "$line" | xargs)

        if [[ "$line" =~ ^Bail\ out ]]; then
            bail_out=true
        elif [[ "$line" =~ ^ok\ [0-9]+\ -\ (.+) ]]; then
            passed=$((passed + 1))
            pass_names+=("${BASH_REMATCH[1]}")
        elif [[ "$line" =~ ^not\ ok\ [0-9]+\ -\ (.+) ]]; then
            failed=$((failed + 1))
            fail_names+=("${BASH_REMATCH[1]}")
        fi
    done <<< "$output"

    if [[ ${#pass_names[@]} -gt 0 ]]; then
        echo -e "${GREEN}PASSED:${NC}"
        for t in "${pass_names[@]}"; do
            echo -e "  ${GREEN}✓ $t${NC}"
        done
    fi

    if [[ ${#fail_names[@]} -gt 0 ]]; then
        echo -e "${RED}FAILED:${NC}"
        for t in "${fail_names[@]}"; do
            echo -e "  ${RED}✗ $t${NC}"
        done
    fi

    echo ""
    echo "═══════════════════════════════════════════════"
    echo "  Passed: $passed  |  Failed: $failed"
    echo "═══════════════════════════════════════════════"

    if $bail_out; then
        fail "Kernel bailed out!"
        return 1
    fi

    if [[ $failed -gt 0 ]]; then
        fail "$failed test(s) FAILED"
        return 1
    fi

    if [[ $passed -eq 0 ]]; then
        warn "No TAP tests found — kernel may not have booted"
        return 1
    fi

    ok "All $passed selftests passed!"
    return 0
}

# ── Main ──────────────────────────────────────────────────────────────

echo ""
echo "╔══════════════════════════════════════════════╗"
echo "║   Vahi Kernel — QEMU + Limine Tests         ║"
echo "╚══════════════════════════════════════════════╝"
echo ""

check_prereqs
build_and_image
run_qemu

RESULT=0
parse_tap "$SERIAL_OUTPUT" || RESULT=$?

exit $RESULT
