# Vahi Kernel Validation Matrix

This document defines the canonical validation gates for the Vahi kernel project.

---

## A. Host/Workspace Validation

| Validation | Command | Target | Purpose |
|------------|---------|--------|---------|
| Format check | `cargo fmt --check` | workspace | Enforce consistent formatting |
| Workspace build | `cargo build --workspace --exclude vahi_kernel` | workspace | Verify all host-buildable crates compile (the kernel builds for its own target in Â§B) |
| Workspace tests | `cargo test --workspace --exclude vahi_kernel` | workspace | Run host-runnable unit tests (the kernel crate's tests execute under QEMU in T-00) |
| Workspace clippy | `cargo clippy --workspace --exclude vahi_kernel -- -D warnings` | workspace | Lint all host-buildable crates with deny-warnings |

**Status**: Format check PASS. Workspace build PASS. Workspace tests PASS (2026-10-21; includes `vahi-types`, formerly excluded). Workspace clippy PASS â€” the only output is vendored `bootloader` build-script warnings, which `-D warnings` does not apply to. The kernel cannot build for the host (E0152) and is verified with its own target in Â§B.

---

## B. Kernel Validation

| Validation | Command | Target | Purpose |
|------------|---------|--------|---------|
| Kernel check | `cargo check --target x86_64-unknown-none` | kernel | Fast compile verification |
| Kernel build (debug) | `cargo build --target x86_64-unknown-none` | kernel | Debug build for development |
| Kernel build (release) | `cargo build --release --target x86_64-unknown-none` | kernel | Optimized production build |
| Kernel clippy | `cargo clippy --target x86_64-unknown-none -- -D warnings` | kernel | Lint kernel with deny-warnings |

**Status**: All PASS (verified 2026-10-21 for default, all-features, release+self_test, and the 19-crate clippy loop). Clippy passes with `-D warnings`; the only output is the workspace profile-location cargo warning (see Â§F) and dependency build-script noise, neither of which `-D warnings` applies to.

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
| vahi-types | `crates/types` | Shared types & trait interfaces | PASS (incl. tests) |
| vahi-limine | `crates/limine` | Limine boot protocol | PASS |
| vahi-crypto | `crates/crypto` | Cryptographic primitives | PASS |
| vahi-acpi | `crates/acpi` | ACPI table parsing | PASS |
| vahi-pci | `crates/pci` | PCI bus enumeration | PASS |
| vahi-gdt | `crates/gdt` | GDT/IDT/TSS management | PASS |
| vahi-ipc | `crates/ipc` | Inter-process communication | PASS |

**Note**: All crates build successfully for `x86_64-unknown-none`. The `vahi-types` test module failures (import scope inside the nested `mod tests`; exclusive `USER_ADDR_MAX` bound) were fixed 2026-10-21; `cargo test -p vahi-types` is 26/26 and CI runs it in the `host-tests` job.

---

## D. Tests

### D.1 Host-runnable unit tests
- `cargo test --workspace --exclude vahi_kernel` â€” Run unit tests in host mode (kernel tests execute under QEMU in T-00, not on the host)
- **Status**: All pass (2026-10-21).

### D.2 Kernel self-tests
- **Entry point**: `tests/run_qemu_tests.py` (ADR-027; supersedes `tests/selftest_gate.sh`)
- **Prerequisites**: Kernel built with `--features self_test`, Limine boot image created
- **Success criterion**: TAP output shows `# N/N passed, 0 failed`; runner classifies PASS and exits 0
- **Timeout**: runner `--timeout` (CI: 2400s)

### D.3 QEMU boot tests
- **Canonical image builder**: `python builder/build_limine_image.py` (Limine path; the `--rust-builder` image cannot boot this kernel â€” FB-004)
- **Boot command**: `qemu-system-x86_64 -drive if=pflash,format=raw,file=OVMF.fd -drive format=raw,file=bootimage-vahi_kernel.bin -serial stdio -m 512M`
- **Success markers**: Limine loading kernel, memory init, frame allocator, heap init, scheduler start

### D.4 Stress/soak tests
- `nightly.yml` `boot-stress` job â€” 25 alternating SMP 1/2 boots (runs nightly)
- `tests/boot_stress_100.ps1`, `tests/smp_stress.ps1` â€” longer local soak scripts (Windows)
- Feature flag build matrix â€” `ci.yml` `kernel-build-matrix` job
- **Status**: Nightly CI keeps a bounded version; the 100-boot soak stays local

---

## E. CI Pipeline Stages

### PR CI (`.github/workflows/ci.yml` â€” every PR and push to `master`)

1. **format** â€” `cargo fmt --all -- --check`
2. **host-tests** â€” `cargo test --workspace --exclude vahi_kernel`
3. **host-clippy** â€” workspace `-D warnings`, kernel excluded
4. **kernel-build-matrix** â€” debug/release/self_test/all-features and solo-feature builds
5. **kernel-clippy** â€” default + all-features + per-crate `-D warnings`
6. **qemu-selftest** â€” Limine image, full T-00 suite via `tests/run_qemu_tests.py` (SMP 1)
7. **qemu-boot-regression** â€” production image boot at SMP 1 and SMP 2 (`--expect-ap 1`)

### Nightly CI (`.github/workflows/nightly.yml` â€” 06:00 UTC + on demand)

1. **full-builds** â€” release builds (default / self_test / all-features)
2. **cross-arch** â€” aarch64 build; riscv64 `cargo check` (no linker script; ADR-028)
3. **selftest-smp2** â€” full T-00 suite at `-smp 2`
4. **selftest-audit** â€” `audit_small` profile suite (overflow-checks on)
5. **boot-stress** â€” 25 alternating SMP 1/2 boots
6. **dependency-audit** â€” `cargo audit` over the workspace lock

### Harness demos (`.github/workflows/t00-demo.yml` â€” on demand)

FAIL / PANIC / TIMEOUT classification proof using `VAHI_SELFTEST_MODE`.

---

## F. Known Baseline Failures

| Failure | Classification | CI Treatment |
|---------|----------------|--------------|
| `vahi-types` test module failures (import scope in nested `mod tests`, exclusive `USER_ADDR_MAX` bound) | FIXED 2026-10-21 | Fix landed; `host-tests` job now includes it |
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
cargo check --workspace --exclude vahi_kernel
cargo test --workspace --exclude vahi_kernel
cargo clippy --workspace --exclude vahi_kernel -- -D warnings

# Kernel validation
cd kernel
cargo check --target x86_64-unknown-none
cargo build --target x86_64-unknown-none
cargo build --release --target x86_64-unknown-none
cargo clippy --target x86_64-unknown-none -- -D warnings

# Self-test gate (requires boot image; run from workspace root)
cd kernel
cargo build --release --target x86_64-unknown-none --features self_test
cd ..
python builder/build_limine_image.py --kernel target/x86_64-unknown-none/release/vahi_kernel --initrd kernel/initrd.tar --output tests/t00_selftest.bin
python tests/run_qemu_tests.py --image tests/t00_selftest.bin --timeout 540 --qemu-extra "-vga none"

# Normal-boot regression
python tests/run_qemu_tests.py --image tests/t00_normal.bin --boot-only --qemu-extra "-vga none"
```
