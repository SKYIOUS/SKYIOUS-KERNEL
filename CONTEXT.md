# CONTEXT — working agreement for this repo (2026-11-07)

Architecture review session. Master plan: `docs/PLAN.md`.
Execution plans: `docs/architecture/implementation-plan-1-deletion-campaign.md` (DONE, committed `5ed4215`), `docs/architecture/implementation-plan-2-wire-up-group-a.md` (next).

## Policy

- Monolithic kernel stays (ADR-001). No new abstractions with one implementation (ADR-009).
- Half-built subsystems: keep + wire up (ash, hypervisor, verified, compositor, ebpf/jit, shell/commands fold) — plan 2.
- Fake/broken subsystems: deleted (vahiai, korlang, kext, rtsched, pressure, ebpf/ash*, hypervisor_tests) — plan 1 done.
- Feature flags: default = `smp,net,ext4,iommu`; CI all-features = `smp,net,ext4,uhci,ash,hypervisor,verification,gpu` (ADR-008).
- Syscalls: ABI frozen (SYSCALL_ABI.md); `syscalls/mod.rs` god file gets split per ADR-010.
- Single kernel clock (ADR-011); shared DMA primitives in `hal/dma.rs` (ADR-012).

## Verification (local, Windows)

- `cargo build` in `kernel/` (pinned nightly from the root `rust-toolchain.toml`; add `C:\Users\nanda\.cargo\bin` to PATH in shells).
- Bootimage: `cargo build --features self_test -Zbuild-std=core,alloc --target x86_64-unknown-none` in `kernel/`, then `cargo run --quiet` in `builder/`.
- QEMU gate: `qemu-system-x86_64 -bios OVMF.fd -drive format=raw,file=target\x86_64-vahi\debug\bootimage-vahi_kernel.bin -m 512M -smp N -nographic -serial file:<log> -no-reboot`; `tests/run_qemu_tests.py --expect-count auto` (count from `tests/selftest_count.txt`) must report 0 failed at smp 1 and 2.
- `scripts/verify.ps1` runs the whole local gate set (fmt, policy, build, clippy, QEMU).
- Clippy `-D warnings` has pre-existing errors in arch/drivers/task/vfs — don't add new ones in touched files.
- `kernel/src/drivers/serial.rs` has an uncommitted pre-existing TX-timeout change — not ours, keep.

## Git

- Commit only explicitly staged files; the tree often carries pre-existing WIP — never `git add -A`.
- One commit per plan step, message style: `kernel: <scope> — <summary>`.

## Known Issues (from docs/architecture/issues.md)

- **P-1**: `vahi-sync` uses x86_64-only asm/intrinsics unconditionally — blocks aarch64/riscv64 builds
- **A-6**: KASLR relocations never applied — ELF magic check fails, kernel boots only via aliasing
- **S-1**: SMP-8 hangs in early boot (last output `[BOOT] syscalls init...`)
- **S-2**: SMP-4 passes all tests then locks up (`[LOCKUP] rip=0xffffffff800374d8 stuck 500 ticks`)
- **F-1**: ext2 directory traversal unbounded (corrupt rec_len walks past block)
- **F-2**: SkyFS extent copy unbounded (disk-supplied extent_count * sizeof(Extent) unchecked)
- **N-1/N-2**: NVMe admin/I/O queues share head/tail/phase; reap discards status
- **A-1/A-2**: AHCI completion wait unbounded; only CTBA header 0 programmed
- **E-1**: E1000 IRQ path allocates heap + takes 4 blocking locks in IRQ context
- **X-2**: xHCI IMAN bit 1 written where bit 0 meant — interrupter never enabled