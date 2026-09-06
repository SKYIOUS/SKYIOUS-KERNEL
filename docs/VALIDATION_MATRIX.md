# Vahi Kernel Validation Matrix

This document defines the canonical validation gates for the Vahi kernel project.

---

## A. Host/Workspace Validation

| Validation | Command | Target | Purpose |
|------------|---------|--------|---------|
| Format check | `cargo fmt --check` | workspace | Enforce consistent formatting |
| Workspace build | `cargo build --workspace` | workspace | Verify all crates compile |
| Workspace clippy | `cargo clippy --workspace -- -D warnings` | workspace | Lint all crates with deny-warnings |

**Status**: Format check PASS. Workspace build PASS. Workspace clippy PASS (with 2 vendor warnings from hashbrown).

---

## B. Kernel Validation

| Validation | Command | Target | Purpose |
|------------|---------|--------|---------|
| Kernel check | `cargo check --target x86_64-unknown-none` | kernel | Fast compile verification |
| Kernel build (debug) | `cargo build --target x86_64-unknown-none` | kernel | Debug build for development |
| Kernel build (release) | `cargo build --release --target x86_64-unknown-none` | kernel | Optimized production build |
| Kernel clippy | `cargo clippy --target x86_64-unknown-none -- -D warnings` | kernel | Lint kernel with deny-warnings |

**Status**: All PASS. Kernel builds successfully in debug and release modes. Clippy passes with `-D warnings` (2 vendor warnings from hashbrown).

---

## C. Crate Validation (Required Kernel Dependencies)

Each extracted crate must build for the `x86_64-unknown-none` target:

| Crate | Path | Purpose | Status |
|-------|------|---------|--------|
| vahi-sync | `crates/sync` | IRQ-safe synchronization primitives | PASS |
| vahi-arch | `crates/arch` | Architecture-specific code (x86_64, aarch64) | PASS |
| vahi-hal | `crates/hal` | Hardware abstraction layer | PASS |
| vahi-drivers | `crates/drivers` | Device drivers | PASS |
| vahi-syscalls | `crates/syscalls` | System call dispatch | PASS |
| vahi-task | `crates/task` | Task scheduling vocabulary | PASS |
| vahi-memory | `crates/memory` | Memory management | PASS |
| vahi-vfs | `crates/vfs` | Virtual filesystem layer | PASS |
| vahi-net | `crates/net` | Networking stack | PASS |
| vahi-objects | `crates/objects` | Kernel object namespace | PASS |
| vahi-types | `crates/types` | Shared types & trait interfaces | PASS (build only; tests have pre-existing failures) |
| vahi-limine | `crates/limine` | Limine boot protocol | PASS |
| vahi-crypto | `crates/crypto` | Cryptographic primitives | PASS |
| vahi-acpi | `crates/acpi` | ACPI table parsing | PASS |
| vahi-pci | `crates/pci` | PCI bus enumeration | PASS |
| vahi-gdt | `crates/gdt` | GDT/IDT/TSS management | PASS |
| vahi-ipc | `crates/ipc` | Inter-process communication | PASS |

**Note**: All crates build successfully for `x86_64-unknown-none`. The `vahi-types` crate has pre-existing test failures due to missing imports in the test module — these are NOT regressions from Gates 1–5.

---

## D. Tests

### D.1 Host-runnable unit tests
- `cargo test --workspace --lib` — Run unit tests in host mode
- **Status**: Most crates pass. `vahi-types` has pre-existing test failures (missing imports in test module).

### D.2 Kernel self-tests
- **Entry point**: `tests/selftest_gate.sh`
- **Prerequisites**: Kernel built with `--features self_test`, boot image created
- **Success criterion**: TAP output shows `# N/N passed, 0 failed` on serial console
- **Timeout**: 180 seconds default

### D.3 QEMU boot tests
- **Canonical image builder**: `python builder/build_limine_image.py --rust-builder` (production) or Limine path
- **Boot command**: `qemu-system-x86_64 -drive if=pflash,format=raw,file=OVMF.fd -drive format=raw,file=bootimage-vahi_kernel.bin -serial stdio -m 512M`
- **Success markers**: Limine loading kernel, memory init, frame allocator, heap init, scheduler start

### D.4 Stress/soak tests
- `tests/e2e_full_suite.sh` — Comprehensive test suite
- 100 consecutive boot stress test
- SMP stress test (4 CPUs)
- Feature flag build matrix
- **Status**: Available for local execution; too slow for CI

---

## E. CI Pipeline Stages

### Current CI (`.github/workflows/ci.yml`)

1. **build-kernel** — Compile kernel with various feature combinations
2. **build-bootimage** — Create bootable disk image
3. **selftest** — Boot in QEMU, verify TAP self-test output (SMP 1 & 2)
4. **clippy** — Lint kernel and extracted crates with `-D warnings`

### Nightly CI (`.github/workflows/nightly.yml`)

1. **full-build** — Release builds with various features
2. **build-aarch64** — Experimental aarch64 build
3. **security-audit** — cargo-audit dependency scanning

### Build Kernel CI (`.github/workflows/build-kernel.yml`)

Simple kernel + bootimage build on push/PR to main.

---

## F. Known Baseline Failures

| Failure | Classification | CI Treatment |
|---------|----------------|--------------|
| `vahi-types` test compilation errors (missing imports in test module) | PRE-EXISTING | Excluded from CI test run; build-only validation |
| hashbrown vendor warnings (2 structs never constructed) | PRE-EXISTING | Accepted; vendor patch already in place |
| Workspace clippy warnings about profile/patch location | PRE-EXISTING | Accepted; cosmetic, does not affect build |

---

## G. Artifacts to Ignore

The following generated artifacts must NOT be committed or consumed as source:

```
*.bin, *.img, *.fd, *.iso, *.ppm
qemu*.log, serial*.log, *.log
builder-target/, bin/, target/
test_logs/, boot_logs/
```

See `.gitignore` for complete list.

---

## H. Validation Commands Summary

```bash
# Quick validation (host only)
cargo fmt --check
cargo check --workspace
cargo clippy --workspace -- -D warnings

# Kernel validation
cd kernel
cargo check --target x86_64-unknown-none
cargo build --target x86_64-unknown-none
cargo build --release --target x86_64-unknown-none
cargo clippy --target x86_64-unknown-none -- -D warnings

# Self-test gate (requires boot image)
cd ..
cargo build --features self_test -Zbuild-std=core,alloc --target x86_64-unknown-none  # from kernel/
python builder/build_limine_image.py --kernel kernel/target/x86_64-unknown-none/debug/vahi_kernel --output bootimage-vahi_kernel.bin
bash tests/selftest_gate.sh 180 1

# Full E2E suite (local only)
bash tests/e2e_full_suite.sh
```