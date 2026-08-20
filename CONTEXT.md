# CONTEXT — working agreement for this repo (2026-08-20)

Architecture review session. Master plan: `docs/architecture/architecture-review-plan.md`.
Execution plans: `docs/architecture/implementation-plan-1-deletion-campaign.md` (DONE, committed `5ed4215`), `docs/architecture/implementation-plan-2-wire-up-group-a.md` (next).

## Policy
- Monolithic kernel stays (ADR-001). No new abstractions with one implementation (ADR-009).
- Half-built subsystems: keep + wire up (ash, hypervisor, verified, compositor, ebpf/jit, shell/commands fold) — plan 2.
- Fake/broken subsystems: deleted (vahiai, korlang, kext, rtsched, pressure, ebpf/ash*, hypervisor_tests) — plan 1 done.
- Feature flags: default = `smp,net,ext4`; CI all-features = `smp,net,ext4,uhci,ash,hypervisor,verification` (gpu pending A4) (ADR-008).
- Syscalls: ABI frozen (SYSCALL_ABI.md); `syscalls/mod.rs` god file gets split per ADR-010.
- Single kernel clock (ADR-011); shared DMA primitives in `hal/dma.rs` (ADR-012).

## Verification (local, Windows)
- `cargo build` in `kernel/` (nightly via rust-toolchain.toml; add `C:\Users\nanda\.cargo\bin` to PATH in shells).
- Bootimage: `cargo build --features self_test -Zbuild-std=core,alloc --target x86_64-unknown-none` in `kernel/`, then `cargo run --quiet` in `builder/`.
- QEMU gate: `qemu-system-x86_64 -bios OVMF.fd -drive format=raw,file=target\x86_64-vahi\debug\bootimage-vahi_kernel.bin -m 512M -smp N -nographic -serial file:<log> -no-reboot`; expect `TAP version 13` + `# 92/92 passed, 0 failed` at smp 1 and 2.
- Clippy `-D warnings` has pre-existing errors in arch/drivers/task/vfs — don't add new ones in touched files.
- `kernel/src/drivers/serial.rs` has an uncommitted pre-existing TX-timeout change — not ours, keep.

## Git
- Commit only explicitly staged files; the tree often carries pre-existing WIP — never `git add -A`.
- One commit per plan step, message style: `kernel: <scope> — <summary>`.