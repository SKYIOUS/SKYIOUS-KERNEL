# ADR-026: Single process/scheduler lineage (kernel-owned), vahi-types as the crate boundary

## Status

Accepted (2026-09-05)

## Context

The workspace carried a **dual lineage**: `kernel/src/task/*` and
`crates/task/src/*` both compiled complete process/thread/scheduler/OOM
implementations. The kernel's copy was the live one (all syscalls, context
switches, and GDT init use it); the crate's copy compiled but its state was
never populated at runtime. ELF evidence before this change:

- two `CURRENT_PROCESS` statics (`vahi_task::process::` + `vahi_kernel::task::process::`)
- two `PER_CPU` scheduler vectors and two `GLOBAL` scheduler instances
- linked code reading the dead statics: `vahi-net` unix sockets read the
  crate's never-written `CURRENT_PROCESS` for SO_PEERCRED and blocked on the
  crate's never-run scheduler (`block_on_pipe`/`wake_pipe`);
  `vahi-drivers` USB slept via the dead scheduler's `this_cpu_sched()`.

The intended dependency-breaking seam already existed in `vahi-types`
(`ProcessProvider` trait + registry) but had **zero callers** — the registry
was never populated, and consumers were wired to the shadow state instead.
Three sibling duplications existed too: per-crate `get_ticks()` extern stubs
(task, drivers, apic — three signatures for one clock), and two whole crates
(`vahi-interrupts`, `vahi-gui`) that nothing consumed and that never reached
the kernel ELF (same dead-lineage pattern as the previously deleted
`kernel/src/memory`).

## Decision

1. **The kernel is the single owner of process identity and scheduling.**
   `kernel/src/task/{process,thread,scheduler}` is the only implementation.
2. **`vahi-types` is the boundary for extracted crates.** The kernel
   implements `ProcessProvider` (plus a new `peer_creds` method) and registers
   it in `scheduler::init()`, together with:
   - `register_tick_fn` — the one monotonic clock (replaces three per-crate
     `vahi_kernel_get_ticks` extern stubs; host builds read 0).
   - `SchedFacade` (`block_on_pipe`/`wake_pipe`) — pipe blocking for `vahi-net`.
   - `register_sleep_facade` (`sleep_until_tick`) — thread-context sleep for
     `vahi-drivers` USB (shared body with `sys_nanosleep`).
   Facades are plain `fn` pointers: no kernel types cross the boundary.
3. **`vahi-task` shrinks to genuinely shared vocabulary**: `pty` (pure byte
   pipes) and the async `Task`/`YieldNow` types. Its process/thread/scheduler/
   oom/elf_dyn/allocator/executor/keyboard/lock/smp modules are deleted.
4. **The context-switch ABI moves into the kernel** (its only user):
   `switch_context` and `fork_child_return` asm + `FORK_CHILD_CS/SS` statics
   now live in `kernel/src/task/thread.rs` (single `#[no_mangle]` definitions).
5. **Dead crates deleted**: `crates/interrupts` and `crates/gui` (zero
   consumers, absent from the ELF) removed from the tree and workspace.

## Consequences

- One `CURRENT_PROCESS`, one `PER_CPU`, one clock — verified in the ELF.
- SO_PEERCRED now resolves through the live per-CPU process identity instead
  of a static that was always `None`/pid 0.
- A future change to process state lands in exactly one file.
- `vahi-task` builds with three dependencies (types, sync, + small ext) and
  compiles in milliseconds.
- The `vahi-types` provider registry is now load-bearing; new crate-side
  needs must extend it rather than re-import kernel state.

## Verification

- `cargo build --release`, `clippy -D warnings`, `fmt --check`: clean.
- TAP selftest boot: **134/134 passed**.
- SMP-4 boots (7x) + single-CPU boots to `login:`, zero kernel panics; the
  only SIGSEGVs are the documented pre-existing userspace login-manager rips
  (`0x403920`, `0x404a6e`, `0x404c90`), one per boot at historical rates.
- `nm vahi_kernel`: single `CURRENT_PROCESS`/`GLOBAL`/`PER_CPU` statics;
  `fork_child_return` defined once.
