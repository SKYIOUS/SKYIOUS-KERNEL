# Implementation Plan: Verification Wiring + Group A Remaining + Syscalls Split

## Overview

Wire the formal verification models (scheduler, journal) into their real runtime
counterparts as assert-only checkpoints under the `verification` feature.
Complete the remaining Group A modules: hypervisor (A2), compositor (A4),
ebpf/jit with executable-memory allocator (A5). Then split the syscalls god
file, deduplicate, and consolidate scheduler/memory (low priority).

## Architecture Decisions

- **Verification is assert-only**: `check_schedule_correctness` and journal
  state-machine checks run only when `verification` feature is enabled; they
  do not alter scheduling decisions or commit behavior. Violations log via
  `VERIFICATION_RUNNER` and serial; no panics.
- **Proof documents moved**: Proof markdown files move from `kernel/src/verified/proofs/`
  to `docs/verified-proofs/` (source-of-truth for audit); inline docs in
  `verified/` modules stay as design references.
- **JIT deferred to A5**: ebpf/jit.rs already compiles (always-on); A5 adds
  a kernel executable-memory allocator (W^X discipline: RW→RX flip) and routes
  ash runtime through it when available; interpreter remains fallback.
- **Syscalls split (ADR-010)**: `syscalls/mod.rs` → `syscalls/fs.rs`,
  `syscalls/process.rs`, `syscalls/net.rs`, `syscalls/ipc.rs`,
  `syscalls/gui.rs`, `syscalls/misc.rs` + thin dispatch in `syscalls/mod.rs`.
  No new abstractions — just file organization per ADR-009.

## Task List

### Phase 1: A3 — Verification Wiring (verification feature)

- [ ] **Task 1**: Move proof documents
  - Move `kernel/src/verified/proofs/*.md` → `docs/verified-proofs/`
  - Update any internal references (none expected)
  - **Files**: `docs/verified-proofs/lock_proof.md`, `scheduler_proof.md`, `journal_proof.md`, `interrupt_proof.md`

- [ ] **Task 2**: Wire scheduler `pick_next` → `check_schedule_correctness`
  - In `kernel/src/task/scheduler.rs::pick_next`, after selecting a thread (either from pending queue, stride heap, or work-stealing), build a `SchedSnapshot` with the ready threads that were considered, the selected index, and `crate::interrupts::get_ticks()` as elapsed_ticks.
  - Call `crate::verified::scheduler::check_schedule_correctness(&snap)` inside `#[cfg(feature = "verification")]`.
  - On violation, log via `crate::verified::runner::VERIFICATION_RUNNER.lock().record_failure("scheduler::pick_next", &format!("{:?}", violation))`.
  - **Files**: `kernel/src/task/scheduler.rs`

- [ ] **Task 3**: Wire SkyFS journal `commit_transaction` → `JournalStateMachine`
  - In `kernel/src/vfs/skyfs/journal.rs::commit_transaction`, after writing the commit marker (state=2 + checksum), call `JournalStateMachine::apply(JournalEvent::TxnPersisted)` inside `#[cfg(feature = "verification")]`.
  - Also wire `begin_transaction` → `BeginTxn`, `recover_from_dev` → `Crash` + `RecoveryComplete`.
  - Violations log via verification runner.
  - **Files**: `kernel/src/vfs/skyfs/journal.rs`

- [ ] **Task 4**: Enable verification runner at boot
  - In `kernel/src/main.rs` (already has `ash::manager::init()` under `#[cfg(feature = "ash")]`), ensure `verified::runner::VERIFICATION_RUNNER` is initialized (it's a static const) and optionally enable via a sysctl or boot flag later.
  - **Files**: `kernel/src/main.rs` (minor)

- [ ] **Task 5**: Add verification selftest
  - In `kernel/src/tests/new_features.rs`, add a test that enables the runner, triggers a scheduler pick, and asserts no violations.
  - Register under `#[cfg(feature = "verification")]`.
  - **Files**: `kernel/src/tests/new_features.rs`

### Checkpoint: A3 Complete
- [ ] `cargo build --features verification` passes
- [ ] QEMU selftest with `--features self_test,verification` passes (94/94 or 95/95 with new test)
- [ ] Verification runner reports visible in serial output

---

### Phase 2: A2 — Hypervisor End-to-End (hypervisor feature)

- [ ] **Task 6**: Consolidate VM model (keep `Hypervisor`+`GuestVm`+`Vcpu`, delete `vmm.rs`/`guest.rs` or alias)
- [ ] **Task 7**: `create_guest` → allocate guest memory + map into EPT (use `ept.rs`)
- [ ] **Task 8**: `vcpu.run()` → persist `vmcs_phys` across runs; remove per-call `VmxHandler::new()`
- [ ] **Task 9**: Fill stub syscalls: `sys_vm_load_kernel` (ENOSYS or ELF+EPT), `sys_vm_set_memory` (EPT map)
- [ ] **Task 10**: Re-add hypervisor selftest (was deleted in plan 1)
- [ ] **Task 11**: Document "hello guest" userspace example in `examples/`

### Checkpoint: A2 Complete
- [ ] `cargo build --features hypervisor` passes
- [ ] Selftest asserting EPT maps a guest page passes

---

### Phase 3: A4 — Compositor Integration (gpu feature)

- [ ] **Task 12**: Define compositor seam: `compositor::compose(&scene)` called from gui frame loop instead of raw `flip()`
- [ ] **Task 13**: Keep software flip path as fallback when `gpu` feature off
- [ ] **Task 14**: Add selftest: `compose(empty scene)` leaves backbuffer unchanged
- [ ] **Task 15**: Restore `gpu` to CI `all-features` list (ci.yml, nightly.yml, Cargo.toml)

### Checkpoint: A4 Complete
- [ ] `cargo build --features gpu` passes
- [ ] QEMU visual smoke test works
- [ ] CI all-features includes `gpu`

---

### Phase 4: A5 — ebpf/jit with Exec-Memory Allocator (always-on)

- [ ] **Task 16**: Add kernel executable-memory allocator (W^X: alloc RW pages, `flip_to_rx` to make RX, never RWX)
- [ ] **Task 17**: Route ash runtime through `ebpf::jit` when JIT-compiled code exists; fallback to interpreter
- [ ] **Task 18**: Remove any remaining NX-violating transmute patterns

### Checkpoint: A5 Complete
- [ ] `cargo build --features ash` passes with JIT path available
- [ ] Selftest covers JIT execution path

---

### Phase 5: Syscalls Split + Dedup + Scheduler/Memory Consolidation (low)

- [ ] **Task 19**: Split `syscalls/mod.rs` into `fs.rs`, `process.rs`, `net.rs`, `ipc.rs`, `gui.rs`, `misc.rs` per ADR-010
- [ ] **Task 20**: Thin dispatch in `syscalls/mod.rs` (match on number → delegate)
- [ ] **Task 21**: Deduplicate similar syscall handlers (e.g., open/openat, read/readv, etc.)
- [ ] **Task 22**: Consolidate scheduler data structures (PerCpuScheduler + GlobalScheduler → unified?)
- [ ] **Task 23**: Consolidate memory allocators (frame allocator + buddy + page cache)

### Checkpoint: Phase 5 Complete
- [ ] All builds pass (default + all-features)
- [ ] QEMU selftest 93/93+ passes
- [ ] No new clippy warnings in touched files

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Scheduler verification adds overhead in hot path | High | Assert-only, zero-cost when feature off; consider sampling (every N picks) if needed |
| Journal verification crashes on violation | Medium | Use `record_failure` (non-panic), log to serial; kernel continues |
| Hypervisor EPT mapping complexity | High | Incremental: map 1 page first, test, then full guest memory |
| Compositor seam breaks existing gui | Medium | Keep software path as default; compositor only when gpu feature on |
| JIT exec-memory allocator NX issues | High | Strict W^X: separate RW and RX pages; test on real hardware + QEMU |
| Syscalls split breaks ABI | High | Thin dispatch preserves exact syscall numbers/ABI; test with selftest |

## Open Questions

- Should scheduler verification sample every pick or every N picks to reduce overhead?
- Does hypervisor need a minimal userspace guest binary in `examples/` for the selftest, or can it be a static kernel payload?
- Compositor: does the existing `virtio_gpu` driver provide enough for HW compositing, or is a new driver needed?
- Syscalls split: should `syscalls/numbers.rs` stay as single source of truth, or split too?

---

**Next immediate action**: Start Task 1 (move proofs) and Task 2 (wire scheduler pick_next).