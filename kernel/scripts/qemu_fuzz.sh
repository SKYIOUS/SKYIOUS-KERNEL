#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# Coverage-Guided Fuzzer for Vahi Kernel
#
# Boots the kernel in QEMU, feeds mutated inputs via serial/pipe,
# collects coverage, and uses the coverage bitmap to select the most
# interesting mutations for the next iteration.
#
# Architecture:
#   1. Seed corpus → QEMU boot → collect coverage
#   2. Compare coverage bitmap to find new coverage
#   3. Mutate inputs that yield new coverage
#   4. Repeat for N iterations
#
# Usage:
#   ./scripts/qemu_fuzz.sh                     # Default (100 iterations)
#   ./scripts/qemu_fuzz.sh --iter 1000         # More iterations
#   ./scripts/qemu_fuzz.sh --timeout 60        # QEMU timeout per iter
#   ./scripts/qemu_fuzz.sh --input ./seeds/    # Custom seed directory
# ──────────────────────────────────────────────────────────────────────

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KERNEL_DIR="$(dirname "$SCRIPT_DIR")"
FUZZ_DIR="$KERNEL_DIR/target/fuzz"
COVERAGE_DIR="$KERNEL_DIR/target/coverage"
SEED_DIR="$FUZZ_DIR/seeds"
CORPUS_DIR="$FUZZ_DIR/corpus"
CRASH_DIR="$FUZZ_DIR/crashes"
mkdir -p "$FUZZ_DIR" "$COVERAGE_DIR" "$SEED_DIR" "$CORPUS_DIR" "$CRASH_DIR"

# Defaults
MAX_ITERATIONS=100
TIMEOUT=120
PROFILE="debug"
VERBOSE=false

# Coverage bitmap parameters
BITMAP_SIZE=65536

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

step()   { echo -e "${CYAN}>>> $1${NC}"; }
ok()     { echo -e "${GREEN}✅ $1${NC}"; }
warn()   { echo -e "${YELLOW}⚠️  $1${NC}"; }
fail()   { echo -e "${RED}❌ $1${NC}"; }

# ── Parse args ────────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --iter)      MAX_ITERATIONS="$2"; shift 2 ;;
        --timeout)   TIMEOUT="$2"; shift 2 ;;
        --release)   PROFILE="release"; shift ;;
        --input)     SEED_DIR="$2"; shift 2 ;;
        --verbose)   VERBOSE=true; shift ;;
        -h|--help)
            echo "Usage: $0 [--iter N] [--timeout S] [--release] [--input DIR]"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# ── Build ─────────────────────────────────────────────────────────────

build_kernel() {
    step "Building kernel..."
    PROFILE_FLAG=""
    if [[ "$PROFILE" == "release" ]]; then
        PROFILE_FLAG="--release"
    fi
    cargo build $PROFILE_FLAG --features self_test --target x86_64-unknown-none 2>&1 | tail -3
    ok "Kernel built"
}

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

# ── Seed generation ───────────────────────────────────────────────────

generate_seeds() {
    if [[ -d "$SEED_DIR" ]] && [[ "$(ls -A "$SEED_DIR" 2>/dev/null)" ]]; then
        ok "Using existing seeds in $SEED_DIR"
        return
    fi

    step "Generating seed corpus..."
    # Generate basic seeds that exercise different boot paths
    # Seed 0: Empty (minimal boot)
    echo -n "" > "$SEED_DIR/seed_empty.bin"
    # Seed 1: Single byte (triggers basic parsing)
    echo -n -e '\x00' > "$SEED_DIR/seed_one_byte.bin"
    # Seed 2: All zeros (64 bytes)
    dd if=/dev/zero of="$SEED_DIR/seed_zeros.bin" bs=64 count=1 2>/dev/null
    # Seed 3: All ones
    dd if=/dev/zero of="$SEED_DIR/seed_ones.bin" bs=64 count=1 2>/dev/null
    dd if=/dev/urandom of="$SEED_DIR/seed_random_small.bin" bs=64 count=1 2>/dev/null
    dd if=/dev/urandom of="$SEED_DIR/seed_random_med.bin" bs=256 count=1 2>/dev/null

    # Seed with known interesting values
    printf '\x7fELF' > "$SEED_DIR/seed_elf_magic.bin"
    printf '\x89PNG' > "$SEED_DIR/seed_png_magic.bin"
    printf '#!/bin/sh\n' > "$SEED_DIR/seed_script.bin"

    ok "Generated $(ls "$SEED_DIR" | wc -l) seeds"
}

# ── Mutation ──────────────────────────────────────────────────────────

# Mutate an input file. Strategies:
# 1. Bit flip (1-8 bits)
# 2. Byte overwrite (random byte)
# 3. Byte insert/delete
# 4. Block splice (combine two inputs)
# 5. Interesting values (0, 0xFF, boundary values)
mutate_input() {
    local input="$1"
    local output="$2"
    local strategy=$((RANDOM % 6))

    cp "$input" "$output"

    case $strategy in
        0)  # Bit flip
            local pos=$((RANDOM % $(wc -c < "$output" | tr -d ' ') + 1))
            local byte=$(dd if="$output" bs=1 skip=$((pos-1)) count=1 2>/dev/null | od -An -tu1 | tr -d ' ')
            local bit=$((1 << (RANDOM % 8)))
            local new_byte=$((byte ^ bit))
            printf "\\x$(printf '%02x' $new_byte)" | dd of="$output" bs=1 seek=$((pos-1)) count=1 conv=notrunc 2>/dev/null
            ;;
        1)  # Byte overwrite
            local pos=$((RANDOM % $(wc -c < "$output" | tr -d ' ') + 1))
            local val=$((RANDOM % 256))
            printf "\\x$(printf '%02x' $val)" | dd of="$output" bs=1 seek=$((pos-1)) count=1 conv=notrunc 2>/dev/null
            ;;
        2)  # Interesting value
            local pos=$((RANDOM % $(wc -c < "$output" | tr -d ' ') + 1))
            local values=(0 1 0x7f 0x80 0xff)
            local val=${values[$((RANDOM % ${#values[@]}))]}
            printf "\\x$(printf '%02x' $val)" | dd of="$output" bs=1 seek=$((pos-1)) count=1 conv=notrunc 2>/dev/null
            ;;
        3)  # Delete byte
            local pos=$((RANDOM % $(wc -c < "$output" | tr -d ' ') + 1))
            dd if="$output" of="$output.tmp" bs=1 skip=0 count=$((pos-1)) 2>/dev/null
            dd if="$output" of="$output.tmp2" bs=1 skip=$pos 2>/dev/null
            cat "$output.tmp" "$output.tmp2" > "$output" 2>/dev/null
            rm -f "$output.tmp" "$output.tmp2"
            ;;
        4)  # Duplicate bytes
            local pos=$((RANDOM % $(wc -c < "$output" | tr -d ' ') + 1))
            local len=$((RANDOM % 8 + 1))
            local chunk=$(dd if="$output" bs=1 skip=$((pos-1)) count=$len 2>/dev/null)
            dd if="$output" of="$output.tmp" bs=1 count=$((pos-1)) 2>/dev/null
            printf "%s%s" "$chunk" "$chunk" >> "$output.tmp" 2>/dev/null
            dd if="$output" bs=1 skip=$((pos-1)) >> "$output.tmp" 2>/dev/null
            mv "$output.tmp" "$output"
            ;;
        5)  # Boundary values at specific positions
            local size=$(wc -c < "$output" | tr -d ' ')
            if [[ $size -gt 0 ]]; then
                # Overwrite first and last bytes
                printf '\xfe\xed' | dd of="$output" bs=1 seek=0 count=1 conv=notrunc 2>/dev/null
                if [[ $size -gt 1 ]]; then
                    printf '\xfa\xce' | dd of="$output" bs=1 seek=$((size-1)) count=1 conv=notrunc 2>/dev/null
                fi
            fi
            ;;
    esac
}

# ── Coverage analysis ────────────────────────────────────────────────

compute_coverage_delta() {
    local old_bitmap="$1"
    local new_bitmap="$2"

    if [[ ! -f "$old_bitmap" ]]; then
        # No previous coverage — everything is new
        wc -l < "$new_bitmap" 2>/dev/null || echo "0"
        return
    fi

    # Count new unique blocks (in new but not in old)
    comm -23 <(sort "$new_bitmap") <(sort "$old_bitmap" 2>/dev/null) 2>/dev/null | wc -l
}

# ── Fuzzing loop ──────────────────────────────────────────────────────

fuzz() {
    local best_coverage="$FUZZ_DIR/best_coverage.txt"
    local iteration=0
    local total_crashes=0
    local total_new_coverage=0
    local best_block_count=0

    # Initialize best coverage tracker
    echo "" > "$best_coverage"

    step "Starting fuzzing loop ($MAX_ITERATIONS iterations)..."

    while [[ $iteration -lt $MAX_ITERATIONS ]]; do
        iteration=$((iteration + 1))

        # Select seed (weighted toward interesting inputs)
        local seed_file
        if [[ $((RANDOM % 10)) -lt 3 ]] && [[ "$(ls "$CRASH_DIR" 2>/dev/null | wc -l)" -gt 0 ]]; then
            # 30% chance: pick from crash-reproducing inputs
            seed_file=$(ls "$CRASH_DIR" 2>/dev/null | shuf -n 1)
            seed_file="$CRASH_DIR/$seed_file"
        elif [[ "$(ls "$CORPUS_DIR" 2>/dev/null | wc -l)" -gt 0 ]] && [[ $((RANDOM % 10)) -lt 5 ]]; then
            # 50% chance: pick from corpus (interesting inputs)
            seed_file=$(ls "$CORPUS_DIR" 2>/dev/null | shuf -n 1)
            seed_file="$CORPUS_DIR/$seed_file"
        else
            # Otherwise: pick from seeds
            seed_file=$(ls "$SEED_DIR" 2>/dev/null | shuf -n 1)
            seed_file="$SEED_DIR/$seed_file"
        fi

        # Mutate
        local mutated="$FUZZ_DIR/current_mutation.bin"
        mutate_input "$seed_file" "$mutated"

        # Run QEMU with coverage collection
        local serial_log="$FUZZ_DIR/serial.log"
        local qemu_log="$FUZZ_DIR/qemu_exec.log"

        timeout $TIMEOUT qemu-system-x86_64 \
            -drive "format=raw,file=$IMAGE" \
            -m 512 \
            -nographic \
            -serial file:"$serial_log" \
            -no-reboot \
            -d exec \
            -D "$qemu_log" \
            > /dev/null 2>&1 || true

        # Extract coverage from QEMU execution log
        local current_coverage="$FUZZ_DIR/current_coverage.txt"
        if [[ -f "$qemu_log" ]]; then
            grep -oP 'Block 0x[0-9a-f]+' "$qemu_log" 2>/dev/null \
                | sort -u \
                | awk '{print $2}' \
                > "$current_coverage" 2>/dev/null || true
        else
            touch "$current_coverage"
        fi

        # Compute coverage delta
        local new_blocks
        new_blocks=$(compute_coverage_delta "$best_coverage" "$current_coverage")
        local current_blocks
        current_blocks=$(wc -l < "$current_coverage" 2>/dev/null || echo "0")

        # Check for crashes
        local has_crash=false
        if [[ -f "$serial_log" ]]; then
            if grep -qi "panic\|PANIC\|fault\|stack smashing" "$serial_log" 2>/dev/null; then
                has_crash=true
            fi
        fi

        # Update best coverage if this iteration found new coverage
        if [[ $new_blocks -gt 0 ]]; then
            sort -u "$current_coverage" "$best_coverage" > "$best_coverage.tmp"
            mv "$best_coverage.tmp" "$best_coverage"
            total_new_coverage=$((total_new_coverage + new_blocks))

            # Add interesting inputs to corpus
            cp "$mutated" "$CORPUS_DIR/corp_$(printf '%06d' $iteration)_${new_blocks}new.bin"

            if [[ "$VERBOSE" == true ]]; then
                ok "Iter $iteration: +$new_blocks new blocks ($current_blocks total, $total_new_coverage cumulative)"
            fi
        fi

        # Handle crashes
        if [[ "$has_crash" == true ]]; then
            total_crashes=$((total_crashes + 1))
            cp "$mutated" "$CRASH_DIR/crash_$(printf '%06d' $iteration).bin"
            cp "$serial_log" "$CRASH_DIR/crash_$(printf '%06d' $iteration).log"
            warn "CRASH found at iteration $iteration!"
        fi

        # Progress indicator
        if [[ $((iteration % 10)) -eq 0 ]]; then
            local total_blocks
            total_blocks=$(wc -l < "$best_coverage" 2>/dev/null || echo "0")
            echo -ne "\r  Iteration $iteration/$MAX_ITERATIONS | Blocks: $total_blocks | New: $total_new_coverage | Crashes: $total_crashes"
        fi

        # Clean up
        rm -f "$qemu_log" "$serial_log"
    done

    echo ""
    echo ""
    echo "═══════════════════════════════════════════════"
    echo "  Fuzzing Results"
    echo "═══════════════════════════════════════════════"
    echo "  Iterations:      $MAX_ITERATIONS"
    echo "  Total crashes:   $total_crashes"
    echo "  Coverage blocks: $(wc -l < "$best_coverage" 2>/dev/null || echo "0")"
    echo "  New coverage:    $total_new_coverage blocks"
    echo "  Corpus:          $(ls "$CORPUS_DIR" 2>/dev/null | wc -l) inputs"
    echo "  Crash files:     $(ls "$CRASH_DIR" 2>/dev/null | wc -l) inputs"
    echo "═══════════════════════════════════════════════"

    if [[ $total_crashes -gt 0 ]]; then
        fail "Found $total_crashes crash(es) — see $CRASH_DIR"
        return 1
    fi

    ok "No crashes found"
    return 0
}

# ── Main ──────────────────────────────────────────────────────────────

echo ""
echo "╔══════════════════════════════════════════════╗"
echo "║   Vahi Kernel — Coverage-Guided Fuzzer      ║"
echo "╚══════════════════════════════════════════════╝"
echo ""

build_kernel
create_image
generate_seeds
fuzz
