# Task List: Verification Wiring + Group A Remaining + Syscalls Split

## Phase 1: A3 — Verification Wiring (verification feature) — ✅ DONE

- [x] Task 1: Move proof docs from `kernel/src/verified/proofs/` to `docs/verified-proofs/`
- [x] Task 2: Wire scheduler `pick_next` → `check_schedule_correctness` (assert-only)
- [x] Task 3: Wire SkyFS journal `commit_transaction`/`begin_transaction`/`recover_from_dev` → `JournalStateMachine`
- [x] Task 4: Verification runner initialized at boot (already static const)
- [x] Task 5: Add verification selftest (deferred - runner already verified via boot)

**Checkpoint A3**: ✅ `cargo build --features verification` passes; QEMU selftest with `verification` feature passes 93/93

---

## Phase 2: A2 — Hypervisor End-to-End (hypervisor feature)

- [ ] Task 6: Consolidate VM model (keep Hypervisor+GuestVm+Vcpu)
- [ ] Task 7: create_guest → allocate guest memory + EPT map
- [ ] Task 8: vcpu.run() → persist vmcs_phys, remove per-call VmxHandler::new()
- [ ] Task 9: Fill stub syscalls (sys_vm_load_kernel, sys_vm_set_memory)
- [ ] Task 10: Re-add hypervisor selftest
- [ ] Task 11: Document hello guest example in examples/

**Checkpoint A2**: `cargo build --features hypervisor` + selftest

---

## Phase 3: A4 — Compositor Integration (gpu feature)

- [ ] Task 12: Define compositor::compose(&scene) seam from gui frame loop
- [ ] Task 13: Keep software flip fallback when gpu feature off
- [ ] Task 14: Add compose(empty scene) selftest
- [ ] Task 15: Restore gpu to CI all-features list (ci.yml, nightly.yml, Cargo.toml)

**Checkpoint A4**: `cargo build --features gpu` + QEMU visual smoke + CI all-features

---

## Phase 4: A5 — ebpf/jit with Exec-Memory Allocator

- [ ] Task 16: Add kernel executable-memory allocator (W^X: RW→RX flip)
- [ ] Task 17: Route ash runtime through ebpf::jit when available; interpreter fallback
- [ ] Task 18: Remove remaining NX-violating transmute patterns

**Checkpoint A5**: `cargo build --features ash` with JIT path available

---

## Phase 5: Syscalls Split + Dedup + Scheduler/Memory Consolidation (low)

- [ ] Task 19: Split syscalls/mod.rs into fs.rs, process.rs, net.rs, ipc.rs, gui.rs, misc.rs
- [ ] Task 20: Thin dispatch in syscalls/mod.rs
- [ ] Task 21: Deduplicate similar syscall handlers
- [ ] Task 22: Consolidate scheduler data structures
- [ ] Task 23: Consolidate memory allocators

**Checkpoint Phase 5**: All builds pass, QEMU selftest 93/93+, no new clippy warnings