#!/usr/bin/env bash
# dev.sh — Daily development loop: fmt → policy → build → clippy → host tests
# Usage: ./scripts/dev.sh [--quick]  (--quick skips host tests)

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KERNEL_DIR="$ROOT_DIR/kernel"
QUICK=0

if [[ "${1:-}" == "--quick" ]]; then
    QUICK=1
fi

PYTHON="python3"
command -v python3 >/dev/null || PYTHON="py"

export CARGO_TERM_COLOR=always

echo "=== fmt ==="
cargo fmt --all -- --check

echo "=== policy self-test ==="
"$PYTHON" "$ROOT_DIR/scripts/check_repo_policy.py" --self-test

echo "=== policy (worktree) ==="
"$PYTHON" "$ROOT_DIR/scripts/check_repo_policy.py" --worktree

echo "=== build (release, self_test) ==="
cd "$KERNEL_DIR"
cargo build --release --target x86_64-unknown-none --features "self_test,smp,net,ext4,iommu" --locked

echo "=== clippy (release, self_test) ==="
cargo clippy --release --target x86_64-unknown-none --features "self_test,smp,net,ext4,iommu" --locked -- -D warnings

if [[ $QUICK -eq 0 ]]; then
    echo "=== host tests (default features) ==="
    cd "$ROOT_DIR"
    cargo test --workspace --exclude vahi_kernel --locked

    echo "=== host tests (all features) ==="
    cargo test --workspace --exclude vahi_kernel --all-features --locked

    echo "=== host clippy (all features) ==="
    cargo clippy --workspace --exclude vahi_kernel --all-targets --all-features --locked -- -D warnings
fi

echo "=== All dev gates passed ==="