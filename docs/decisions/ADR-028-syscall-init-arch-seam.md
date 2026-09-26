# ADR-028: Syscall-Entry Init Through the `Arch::init_syscalls()` Seam

## Status
Accepted — x86_64 wiring implemented & verified (`refactor/syscall-hygiene`);
aarch64/riscv64 wiring and the `boot/init.rs:138` portability fix are
**deferred** pending cross-target (aarch64-unknown-none / riscv64) toolchain +
QEMU verification, which is not available in this x86_64-only environment.

## Date
2026-09-26

## Context
`kernel/src/arch/mod.rs:17` declares an arch-specific syscall-entry init seam,
`unsafe fn init_syscalls();`, with per-arch impls:

- `arch/arch_x86_64.rs:21-23` -> `crate::syscalls::init()` (Star/Lstar/SFMask/Efer
  MS-Rs + GS-base + SMAP).
- `arch/arch_aarch64.rs:21-24` -> `init_vector_table()` (real vector-table setup).
- `arch/arch_riscv64.rs:12-14` -> `unimplemented!("RISC-V not yet supported")`.

Before this decision the seam had **zero callers**. Boot instead hard-coded the
x86 MS-R path in two places:

- `arch/arch_x86_64.rs:17` inside `init_boot()`.
- `kernel/src/boot/init.rs:138` inside `init_architecture()`, itself gated
  `#![cfg(not(target_arch = "aarch64"))]` — i.e. it runs on x86_64 **and riscv64**,
  so riscv64 would execute x86 MS-R writes that do not compile/encode on riscv64
  (a portability bug masked only because riscv64 isn't built in CI).

Consequences of the dead seam:

- aarch64 `init_vector_table()` never runs -> aarch64 syscall entry is silently
  uninitialized (SIGILL / entry fault on the first syscall).
- riscv64 `init_syscalls()` is `unimplemented!()`; even if wired it traps.
- x86_64 survives only because `init_boot()` hard-codes the x86 path, bypassing
  the `Arch` abstraction — an architecture-boundary leak
  (`AGENTS.md` §4: x86_64-specific code must be `#cfg`-guarded; the arch seam is
  the correct choke point).

This surfaced during the thermo-nuclear syscall-hygiene review (Candidate 8).

## Decision
Route syscall-entry init through `CurrentArch::init_syscalls()` as the single
source of truth, invoked from BSP boot. Replace the hard-coded x86 MS-R call in
`x86_64 init_boot()` with `Self::init_syscalls()`.

```
// x86_64 (DONE, verified)
init_boot() {
    ...
    unsafe { Self::init_syscalls() };   // was: crate::syscalls::init();
    ...
}
// aarch64 / riscv64 (TODO): add `Self::init_syscalls();` to their init_boot()
// after cross-target verification.
```

On x86_64 behavior is identical: the impl still calls `crate::syscalls::init()`.
APs are unaffected (they use the per-CPU path in `kernel/src/smp.rs:388-407`
`init_gs_base`/`init_syscall_msrs`, never `init_boot`).

Two follow-ups are part of this decision's scope and must land before merge:

1. **riscv64 `init_syscalls()` stub** must become a no-op (not `unimplemented!()`)
   so BSP boot does not trap when the seam is wired.
2. **`boot/init.rs:138`** must stop hard-coding `crate::syscalls::init()` under
   `cfg(not(target_arch = "aarch64"))`; either drop it (let arch `init_boot` own
   the call) or re-gate to `cfg(target_arch = "x86_64")`.

## Alternatives Considered

### (b) Delete the dead `Arch::init_syscalls()` seam + 3 impls
- Pros: trivial, zero behavior change (dead code today).
- Cons: deletes the intended arch seam and the aarch64 `init_vector_table()`
  logic that a future port needs; violates `ADR-009` (keep wire-up seams) and
  leaves the `boot/init.rs:138` portability bug untouched.
- Rejected as the long-term design; accepted as a fallback if cross-target
  verification cannot be obtained.

## Consequences
- Removes the arch-boundary leak: non-x86 ports add a single `init_syscalls()`
  impl instead of being papered over by `cfg(not(aarch64))` MS-R calls.
- Makes aarch64 vector-table init explicit and testable (aarch64 BSP will run
  `init_vector_table()` once wired).
- `riscv64` syscall entry is explicitly unsupported (no-op) instead of
  `unimplemented!()`.
- **Requires cross-target build + QEMU verification** for aarch64 and riscv64
  before the deferred follow-ups can merge. This ADR blocks those merges.
- `tests/run_qemu_tests.py` should gain aarch64/riscv64 boot cases once the
  toolchain is available (see `ADR-027` for the exit-code protocol).

## Evidence
- `cargo clippy -p vahi_kernel --target x86_64-unknown-none --no-default-features
  --features "smp,net,ext4,iommu,uhci,ash,hypervisor,verification,gpu" -- -D
  warnings` -> exit 0 after the x86_64 wiring (18 crates + kernel clean).
- Callers of `crate::syscalls::init()`: `arch_x86_64.rs:17` (init_boot),
  `arch_x86_64.rs:22` (init_syscalls impl), `boot/init.rs:138` (init_architecture).
  Callers of `CurrentArch::init_syscalls()`: none (dead) before this decision;
  x86_64 `init_boot` after.
