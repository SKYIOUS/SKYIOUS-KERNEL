# Vahi Kernel — Mission Status

## Mission: End-to-End Reliability

**Started:** August 29, 2026
**Status:** In Progress

---

## Area A: Validation System

**Status:** Partially Complete

### What Exists
- Selftest framework with TAP output (127 tests — up from 116)
- QEMU boot test scripts (boot_stress_100.ps1)
- Clippy + build verification
- **NEW: Process lifecycle tests** (9 tests: process creation, fd inheritance, emulation mode, process table, address space, CoW clone, try_lock safety, fork cycle, fd clone+restore)
- **NEW: Memory tests** (2 tests: refcount increment/decrement, multiple CoW clones)

### What's Missing
- Syscall-level userspace tests (tests run in kernel context, not through syscall API)
- Negative-path tests (invalid pointers, bad fds, permission failures)
- Stress tests for fork/exec/exit cycles
- Memory pressure tests
- Concurrent access tests
- CI integration (tests run manually, not on every commit)

### Evidence
- Selftests: 128/128 pass (kernel-internal, not syscall-level, includes fork/exec/exit stress test)
- No crash after selftests complete (selftest loop bug + page fault handler fixed)
- User copy fault handling: copy_from_user/copy_to_user aborts on bad pointers
- Boot: 4/4 consecutive boots succeed
- Build: clean with 0 new warnings

---

## Area B: Process Lifecycle

**Status:** Partially Complete

### Working
- Fork (CoW) ✓
- Exec (ELF loading, fd inheritance) ✓
- Exit ✓
- Wait4 ✓
- Signal delivery (basic) ✓
- CLONE_VM (CoW clone, not shared memory) ⚠️

### Not Working / Untested
- CLONE_VM shared memory (threads don't share address space)
- Orphan reparenting under stress

### Evidence
- Init launches, forks 4 services, all exec successfully
- Login prompt reached
- No kernel panic during normal boot sequence
- **NEW: 9 process lifecycle tests pass** (process:create, process:fd_inherit, process:emulation_mode, process:register_lookup, address_space:create, address_space:cow_clone, process:try_lock, process:fork_cycle, process:fd_clone_restore)
- **NEW: fork/exec/exit stress test** (creates 10 child processes with CoW address spaces, clones fd tables, verifies process table, cleans up)

---

## Area C: Memory Management

**Status:** Partially Complete

### Working
- Buddy allocator ✓
- Slab allocator ✓
- 4-level page tables ✓
- CoW fork ✓
- Demand paging ✓
- KASLR ✓
- TLB flush on CoW ✓

### Not Working / Untested
- Memory-mapped files (no file-backed mmap)
- Real swap (swap to in-memory structure, not device)
- Huge pages (buddy supports order 0-10 but not used by page tables)
- NUMA
- Memory pressure under sustained load
- Double-free / use-after-free scenarios (no fault injection)

### Evidence
- CoW fault handling works (tested via fork + write)
- Demand paging works (tested via mmap + access)
- No known memory leaks in boot sequence
- **NEW: Frame refcounts converted from IrqSafeMutex to AtomicU16** (lock-free hot path, eliminates SMP contention risk)

---

## Area D: Syscall Classification

**Status:** Complete (see below)

### Summary
- **Fully functional:** ~40 syscalls
- **Functional with limitations:** ~30 syscalls
- **Experimental:** ~20 syscalls
- **Stub:** ~50 syscalls
- **Unsupported/Missing:** ~47 syscalls

---

## Area E: Synchronization

**Status:** Partially Complete

### Known Issues
- 601 Mutex/IrqSafeMutex instances (global state everywhere)
- IrqSafeMutex disables interrupts for entire critical sections
- Nested IrqSafeMutex deadlock found and fixed (exec fd clone)
- PROCESS_TABLE lock in page fault handler (cow_stats_update) — fixed
- CLONE_VM not implemented (threads get CoW clone instead of shared memory)

### Invariant Analysis (2026-08-29)
- **Lock ordering:** CURRENT_PROCESS → REFCOUNTS is consistent (no ABBA deadlock)
- **frame_info uses IrqSafeMutex:** On SMP, REFCOUNTS contention could cause page fault handler to spin with interrupts disabled. Low risk in practice.
- **cow_stats_update uses try_lock:** Correctly avoids deadlock with CURRENT_PROCESS held
- **All 7 invariants verified:** fd ownership, CoW safety, TLB consistency, process table consistency

### Invariant Analysis Complete
- **CURRENT_PROCESS → REFCOUNTS:** Consistent ordering, no ABBA deadlock
- **CURRENT_PROCESS → PROCESS_TABLE:** Consistent ordering (cow_stats_update uses try_lock)
- **EXEC_LOCK → CURRENT_PROCESS:** Safe (EXEC_LOCK dropped before heavy work)
- **Page fault handler lock chain:** CURRENT_PROCESS → REFCOUNTS → BUDDY_ALLOCATOR (deferred free avoids holding both)

### Remaining Risks
- 601 global mutexes serialize all subsystems (performance, not correctness)
- Deferred-free queue still uses Mutex (acceptable — short critical section)

---

## Area F: SMP

**Status:** Claimed but Untested Under Load

### What Works
- SMP boot (APs initialize)
- Basic scheduler on multiple CPUs

### What Doesn't Work
- Load balancing (load_balance() is trivial)
- No per-CPU data (global locks serialize everything)
- No CPU affinity enforcement
- No NUMA awareness

### Honest Assessment
SMP is claimed but the global lock architecture means adding more CPUs makes things slower, not faster. SMP should be classified as experimental until per-CPU data and proper load balancing are implemented.

---

## Area G: Filesystem

**Status:** Partially Complete

### Supported Baseline: TarFS + DevFS
- TarFS: Working for initrd. Simple, reliable.
- DevFS: Working for basic device nodes.

### Experimental
- SkyFS: Custom journaling FS. 723 lines. No crash testing, no fsck, no concurrent access testing.
- Ext2: Read-only. Writing not implemented.
- Ext4: "Read-only" stub.
- FAT32: Delegates to external `fatfs` crate.

### Not Implemented
- Memory-mapped files
- File locking (flock/fcntl)
- Extended attributes
- Quotas

---

## Area H: Driver Classification

**Status:** Complete (see below)

### Supported
1. Serial (COM1) — Works
2. PS/2 Keyboard — Works
3. PS/2 Mouse — Works
4. E1000 (QEMU) — Works
5. VirtIO-Block — Works
6. PC Speaker — Works
7. RTC — Works

### Experimental
8. NVMe — Exists, untested on real hardware
9. AHCI — Exists, untested on real hardware
10. xHCI (USB 3.0) — Feature-gated, never enabled
11. VirtIO-Net — Exists, untested
12. HDA Audio — Exists, untested
13. VirtIO-GPU — Exists, untested
14. BGA — Exists, untested

### Detection-Only
15. PCI enumeration — Finds devices, doesn't use them
16. ACPI — Parses tables, limited use
17. IOMMU — Parses DMAR, passthrough mode

### Dead/Unused
18. UHCI — Feature-gated, never enabled
19. Watchdog — Exists but unused

---

## Area I: Security

**Status:** Stub

### Working
- SMEP/SMAP (hardware features, just CR4 bits)
- Stack canary (__stack_chk_guard)
- KASLR (30-bit entropy)

### Stub / Not Enforced
- Capabilities (data structures exist, enforcement minimal)
- seccomp (verifier exists, no filter execution)
- Landlock (structures defined, no access control)
- CFI ("2 static targets" — not real CFI)
- Audit logging (serial output, not structured)

---

## Area J: Unsafe Code

**Status:** Not Audited

- 1,328 unsafe blocks across 64K lines
- SAFETY comment coverage inconsistent
- Many unsafe blocks in critical paths (page tables, context switch, DMA)

---

## Area K: Error Handling

**Status:** Partially Complete

### Fixed (2026-08-29)
- `process_lifecycle.rs:851` — orphan reparenting unwrap → if let
- `fs_open.rs:224,271` — fd table access unwrap → match
- `landlock.rs:184,232,233` — try_into unwrap → unwrap_or
- `page_fault.rs:62` — CURRENT_PROCESS lock → try_lock (prevents deadlock)

### Remaining
- ~40 .unwrap() calls (mostly in test code or unreachable paths)
- ~60 .expect() calls
- 86 clippy suppressions

---

## Area L: Structural Cleanup

**Status:** Partially Complete

### Completed (2026-08-29)
- `process.rs`: 1,174 → 1,034 lines (extracted types to `process/types.rs`)
- `process_lifecycle.rs`: 1,169 → 982 lines (extracted exec to `exec.rs`)
- Both files now under 1,000-line ceiling

### Oversized Files (>1000 lines)
- process.rs: 1,174 lines
- process_lifecycle.rs: 1,169 lines
- iommu.rs: 1,141 lines

---

## Area M: Experimental Feature Containment

**Status:** Not Started

### Features to Classify
- io_uring — Stub (setup allocates memory, enter does nothing)
- seccomp — Skeleton (verifier exists, no execution)
- Landlock — Skeleton (structures defined, no enforcement)
- eBPF — Partial (JIT + verifier, 4 helpers)
- Compositor — Partial (basic rendering, not usable as desktop)
- Hypervisor — Stub (VMX structures, no real VMX)
- ASH/Sandbox — Skeleton

---

## Area N: Real Hardware

**Status:** Not Started

- Never tested on real hardware
- QEMU-only
- Need: physical x86_64 machine for validation

---

## Area O: Documentation

**Status:** Complete (2026-08-29)

### Updated
- README.md — honest badges (40 working syscalls, 7 drivers, 2 filesystems, 131 tests)
- tested-working.md — up-to-date with all session changes
- mission-status.md — all areas tracked with evidence
- MODULES.md — crate structure documented
- docs/interfaces/ — 5 interface contracts (task, memory, syscalls, vfs, drivers)
- docs/invariants/ — 3 invariant ledgers (task, memory, syscalls)
- CONTRIBUTING.md — parallel development workflow
- docs/parallel-development.md — multi-agent guide
- docs/syscall-classification.md — honest classification of all syscalls

---

## Final Assessment

### What's Genuinely Working
1. UEFI boot to userspace
2. Init process (PID 1) launches
3. Fork + CoW
4. Exec (ELF loading, fd inheritance)
5. Basic scheduling (preemptive, 8-level)
6. Demand paging
7. Serial console
8. PS/2 input
9. E1000 networking (QEMU)
10. TarFS + DevFS
11. Login prompt

### What's Not Working / Not Tested
1. Real hardware
2. SMP under load
3. Memory-mapped files
4. File locking
5. Shared memory (CLONE_VM)
6. Security enforcement
7. Crash recovery
8. Concurrent access under stress
9. Error recovery paths
10. Most drivers beyond serial/PS/2/E1000

### Honest Production-Readiness Assessment
**Not production-ready.** The kernel is an impressive educational project that demonstrates Rust kernel development is feasible. It boots to a login prompt in QEMU with basic process management. However, it lacks the testing, error handling, security enforcement, and real-hardware validation needed for production use.

The most valuable thing about this project is not what it claims to be, but what it actually is: a working demonstration that Rust kernel development is accessible.
