#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# Vahi Kernel — QEMU Integration Test Runner
#
# Builds the kernel, creates a bootable image, launches QEMU,
# captures serial output, and parses TAP results.
#
# Usage:
#   ./scripts/qemu_test.sh                    # Default (debug, 1 CPU)
#   ./scripts/qemu_test.sh --release          # Release build
#   ./scripts/qemu_test.sh --smp 4            # 4-CPU SMP
#   ./scripts/qemu_test.sh --timeout 300      # 5-minute timeout
#   ./scripts/qemu_test.sh --kvm              # Use KVM acceleration
#   ./scripts/qemu_test.sh --ci               # CI mode (strict, exit on fail)
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
CI=false
EXTRA_QEMU_ARGS=""

# ── Parse args ────────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --release)   RELEASE=true; shift ;;
        --smp)       SMP="$2"; shift 2 ;;
        --timeout)   TIMEOUT="$2"; shift 2 ;;
        --kvm)       KVM=true; shift ;;
        --ci)        CI=true; shift ;;
        --qemu-args) EXTRA_QEMU_ARGS="$2"; shift 2 ;;
        -h|--help)
            echo "Usage: $0 [--release] [--smp N] [--timeout S] [--kvm] [--ci]"
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
        fail "qemu-system-x86_64 not found in PATH"
        echo "Install QEMU: https://www.qemu.org/download/"
        exit 1
    fi
    ok "QEMU found: $(which qemu-system-x86_64)"
}

# ── Build kernel ─────────────────────────────────────────────────────

build_kernel() {
    step "Building kernel (self_test feature)..."
    PROFILE_FLAG=""
    if $RELEASE; then
        PROFILE_FLAG="--release"
    fi

    if ! cargo build $PROFILE_FLAG --features self_test --target x86_64-unknown-none 2>&1 | tee "$TEST_DIR/build.log"; then
        fail "Kernel build failed"
        cat "$TEST_DIR/build.log"
        exit 1
    fi
    ok "Kernel built successfully"
}

# ── Create boot image ────────────────────────────────────────────────

create_boot_image() {
    step "Creating bootable image with bootimage..."

    if ! command -v cargo-bootimage &>/dev/null && ! cargo bootimage --help &>/dev/null 2>&1; then
        warn "bootimage not installed. Installing..."
        cargo install bootimage
    fi

    if ! cargo bootimage --no-run 2>&1 | tee "$TEST_DIR/bootimage.log"; then
        fail "bootimage failed"
        cat "$TEST_DIR/bootimage.log"
        echo "Install manually: cargo install bootimage"
        exit 1
    fi

    # Find the generated image
    TARGET_DIR="$KERNEL_DIR/target/x86_64-unknown-none"
    if $RELEASE; then
        TARGET_DIR="$TARGET_DIR/release"
    else
        TARGET_DIR="$TARGET_DIR/debug"
    fi

    BOOT_IMAGE=$(find "$TARGET_DIR" -maxdepth 1 \( -name "bootimage-*.bin" -o -name "*boot_image*.img" \) 2>/dev/null | head -1)

    if [[ -z "$BOOT_IMAGE" ]]; then
        fail "No boot image found in $TARGET_DIR"
        exit 1
    fi

    ok "Boot image: $(basename "$BOOT_IMAGE") ($(du -h "$BOOT_IMAGE" | cut -f1))"
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

    if [[ -n "$EXTRA_QEMU_ARGS" ]]; then
        # shellcheck disable=SC2206
        QEMU_ARGS+=($EXTRA_QEMU_ARGS)
    fi

    SERIAL_LOG="$TEST_DIR/serial.log"
    QEMU_STDERR="$TEST_DIR/qemu_stderr.log"

    # Run QEMU with timeout
    EXIT_CODE=0
    timeout "$TIMEOUT" qemu-system-x86_64 "${QEMU_ARGS[@]}" \
        >"$SERIAL_LOG" 2>"$QEMU_STDERR" || EXIT_CODE=$?

    ELAPSED_START=$(date +%s%N)

    if [[ $EXIT_CODE -eq 124 ]]; then
        warn "QEMU timed out after ${TIMEOUT}s (this may be expected for boot tests)"
    fi

    SERIAL_OUTPUT=$(cat "$SERIAL_LOG" 2>/dev/null || echo "")

    echo ""
    echo -e "${CYAN}─── Serial Output ───${NC}"
    echo "$SERIAL_OUTPUT" | tail -50
    echo -e "${CYAN}─── End Serial ──────${NC}"
    echo ""
}

# ── Parse TAP ─────────────────────────────────────────────────────────

parse_tap() {
    local output="$1"
    local passed=0
    local failed=0
    local version=""
    local planned=""
    local bail_out=false
    local pass_names=()
    local fail_names=()

    while IFS= read -r line; do
        line=$(echo "$line" | xargs)  # trim

        if [[ "$line" =~ ^TAP\ version\ ([0-9]+) ]]; then
            version="${BASH_REMATCH[1]}"
        elif [[ "$line" =~ ^1\.\.([0-9]+) ]]; then
            planned="${BASH_REMATCH[1]}"
        elif [[ "$line" =~ ^Bail\ out ]]; then
            bail_out=true
        elif [[ "$line" =~ ^ok\ [0-9]+\ -\ (.+) ]]; then
            passed=$((passed + 1))
            pass_names+=("${BASH_REMATCH[1]}")
        elif [[ "$line" =~ ^not\ ok\ [0-9]+\ -\ (.+) ]]; then
            failed=$((failed + 1))
            fail_names+=("${BASH_REMATCH[1]}")
        fi
    done <<< "$output"

    # Print results
    if [[ ${#pass_names[@]} -gt 0 ]]; then
        echo -e "${GREEN}PASSED tests:${NC}"
        for t in "${pass_names[@]}"; do
            echo -e "  ${GREEN}✓ $t${NC}"
        done
    fi

    if [[ ${#fail_names[@]} -gt 0 ]]; then
        echo -e "${RED}FAILED tests:${NC}"
        for t in "${fail_names[@]}"; do
            echo -e "  ${RED}✗ $t${NC}"
        done
    fi

    # Summary
    echo ""
    echo "═══════════════════════════════════════════════"
    echo "  TAP Version:  ${version:-N/A}"
    echo "  Tests passed: $passed"
    echo "  Tests failed: $failed"
    echo "  Plan:         1..${planned:-N/A}"
    echo "═══════════════════════════════════════════════"

    # Verdict
    if $bail_out; then
        fail "Kernel bailed out during selftest!"
        return 1
    fi

    if [[ $failed -gt 0 ]]; then
        fail "$failed selftest(s) FAILED"
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
echo "║   Vahi Kernel — QEMU Integration Tests      ║"
echo "╚══════════════════════════════════════════════╝"
echo ""

check_prereqs
build_kernel
create_boot_image
run_qemu

RESULT=0
parse_tap "$SERIAL_OUTPUT" || RESULT=$?

# CI mode: exit with failure
if $CI && [[ $RESULT -ne 0 ]]; then
    echo ""
    fail "CI check failed — see above for details"
    exit $RESULT
fi

exit $RESULT
