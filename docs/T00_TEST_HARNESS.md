# T-00 Test Harness — Vahi Kernel

Status: **Implemented and validated 2026-09-15** (phase T-00). Protocol
decision: [ADR-027](decisions/ADR-027-t00-selftest-protocol.md).

## What T-00 delivers

| Component | Path | Role |
|---|---|---|
| Kernel self-test protocol | `kernel/src/selftest.rs` | TAP 13 on serial + QEMU isa-debug-exit codes |
| Harness demos (pass/fail/timeout/panic) | `kernel/src/tests/harness_tests.rs` | prove the harness detects every result class |
| Panic result path | `kernel/src/panic_handler.rs` | machine-readable panic marker + exit 0x11 (self_test builds) |
| Host runner | `tests/run_qemu_tests.py` | build-free boot→classify loop, 7 result classes |
| CI gate | `.github/workflows/ci.yml` (`t00-harness` job) | full suite on every PR |
| CI demo matrix | `.github/workflows/t00-demo.yml` | on-demand FAIL/TIMEOUT/PANIC proof |

## Running T-00 locally (Windows Git Bash / Linux)

```bash
# 1. Build the self_test kernel (release; ~35s)
cd kernel
cargo build --release --target x86_64-unknown-none --features self_test -Zbuild-std=core,alloc
cd ..

# 2. Build the bootimage (Limine builder; includes initrd)
python builder/build_limine_image.py \
    --kernel target/x86_64-unknown-none/release/vahi_kernel \
    --initrd kernel/initrd.tar \
    --output tests/t00_selftest.bin

# 3. Run the harness (full suite)
python tests/run_qemu_tests.py \
    --image tests/t00_selftest.bin \
    --timeout 540 \
    --qemu-extra "-vga none" \
    --log tests/t00_last_run.log
# -> RESULT: PASS (exit 0) with "# N/N passed, 0 failed" summary
```

Controlled demos (prove the harness fails correctly):

```bash
VAHI_SELFTEST_MODE=fail   cargo build ...   # -> runner exit 1 (FAIL)
VAHI_SELFTEST_MODE=panic  cargo build ...   # -> runner exit 2 (PANIC)
VAHI_SELFTEST_MODE=timeout cargo build ...  # -> runner exit 3 (TIMEOUT)
```

Normal-boot regression check (no self_test kernel needed):

```bash
cargo build --release --target x86_64-unknown-none -Zbuild-std=core,alloc
python builder/build_limine_image.py --kernel ... --output tests/t00_normal.bin
python tests/run_qemu_tests.py --image tests/t00_normal.bin --boot-only \
    --qemu-extra "-vga none" --log tests/t00_boot_run.log
# -> RESULT: PASS once [BOOT] VFS init is reached
```

## Result classes (runner exit codes)

`0` PASS · `1` FAIL · `2` PANIC · `3` TIMEOUT · `4` BOOT_FAILURE ·
`5` UNEXPECTED_EXIT · `6` INFRASTRUCTURE_ERROR — exact conditions in
[ADR-027](decisions/ADR-027-t00-selftest-protocol.md).

## Test protocol summary

Kernel → serial (TAP 13): `TAP version 13`, `1..N`, per-test
`ok/not ok`, summary `# P/N passed, F failed`; panic emits
`Bail out! KERNEL PANIC`. Test mode announces itself first with
`[SELFTEST] T-00 test mode active`. On completion the kernel writes the
isa-debug-exit code (0x10 pass / 0x11 panic) to port 0xf4, so QEMU exits
15/16 — a second, independent confirmation channel.

## Findings register (unrelated defects, recorded not fixed)

| ID | Sev | Subsystem | Evidence | Impact | Future phase |
|---|---|---|---|---|---|
| FB-001 | P0 | build/KASLR | Uncommitted WIP (`kaslr_reloc.rs` etc.) had 3 duplicate `apply_kaslr_relocations` defs + scope bugs; self_test build failed E0428/E0425; `bootimage-vahi_kernel.bin` (Sep 14) triple-faults pre-userspace | No kernel build possible until stashed; WIP preserved in `git stash` + `T00_QUARANTINE/` | K-00 (audit) |
| FB-002 | P0 | memory/boot | VGA text buffer (phys 0xB8000) unmapped in HHDM when it falls in the reserved hole 0x9FC00..0x100000 → write via `VGA_BUFFER_VIRT` triple-faults pre-IDT (`-d int`: `v=0e CR2=ffff8000000b8000` → `v=08` → triple fault) | Silent boot death on some QEMU/OVMF memmaps — **fixed in T-00** (min necessary: map 0xB0000–0xC0000 in `boot/init.rs`) | — |
| FB-003 | P1 | graphics | Splash fill touches first byte past reported FB size: `-d int` shows `v=0e CR2=ffff8000801d5000` = exactly 800×600×4; requires `-vga none` to boot | Every-graphics boot dies with display enabled on QEMU 10.2.50; run + CI use `-vga none` until fixed | K-09 / G-01 |
| FB-004 | P1 | build/boot | Rust builder (`builder/src/main.rs`, bootloader crate) produces an image that can't boot this kernel ("bootloader config section not found"); CI `build-bootimage` job artifacts are unbootable; `selftest_gate.sh` assumes that path | CI gate was structurally unrunnable; `t00-harness` job now builds via Limine builder | T-01 |
| FB-005 | P1 | userspace | Production path dies at OOM during userspace init (`[OOM] FATAL: Cannot reclaim enough memory`, serial `tests/prodimg.log` of the Sep-14 image) | login only reachable with `-vga none` on current QEMU; memory budget unverified | K-03 |

## Known limitations

- Suite runtime varies 10s–10min under TCG depending on host/machine state
  (benchmarks + stress tests dominate); CI budget is 45 min.
- The kernel-side `QEMU_EXIT_TIMEOUT_BASE` (0x30) guard is reserved but
  not yet implemented in `run_all()`; host wall-clock is the hang authority.
- Real-hardware (`-device isa-debug-exit` absent) runs fall back to the
  timeout classification — unchanged from pre-T-00 behavior.
- Runner serial parsing is whole-file-rescan per poll (simple and robust);
  fine for TAP-sized output, revisit if serial volume grows.
