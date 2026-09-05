# Invariants: `task` Module

**Last verified:** 2026-08-29
**Verified by:** Invariant-driven design analysis

---

## Critical Invariants

### INV-T1: PID Uniqueness

**Statement:** Every process has a unique PID. No two processes share a PID at the same time.

**Boundary:** Process creation, process exit

**Enforcement:** `Process::next_id()` uses an atomic counter. PIDs are never recycled within a boot session.

**Verification:**
- Unit test: `process:create` verifies PID assignment
- Code inspection: `next_id()` is `AtomicU64::fetch_add(1, Relaxed)`

**Failure mode:** If PID wraps around (theoretically possible after 2^64 processes), a PID collision could occur. This is not a practical concern.

---

### INV-T2: Parent-Child Consistency

**Statement:** Every process (except PID 1) has a parent in PROCESS_TABLE. Every parent's children list contains only PIDs of processes that exist in PROCESS_TABLE.

**Boundary:** Process creation, process exit, orphan reparenting

**Enforcement:**
- `sys_clone` adds child PID to parent's children list
- `sys_wait4` removes child PID from parent's children list
- `sys_exit` reparents orphans to PID 1

**Verification:**
- Unit test: `process:register_lookup` verifies table consistency
- Code inspection: exit path handles orphan reparenting

**Failure mode:** If `sys_exit` panics after removing from PROCESS_TABLE but before reparenting children, orphans would have invalid parent references. This is mitigated by making exit infallible.

---

### INV-T3: CURRENT_PROCESS Always Set

**Statement:** After boot, CURRENT_PROCESS is always Some. It is only None during early boot (before init process is created).

**Boundary:** Boot, process creation, process exit

**Enforcement:**
- Boot state machine sets CURRENT_PROCESS before entering userspace
- `sys_exit` does NOT clear CURRENT_PROCESS (the thread continues)
- `sys_execve` updates CURRENT_PROCESS to the new process

**Verification:**
- Code inspection: CURRENT_PROCESS is set in boot/state.rs and updated in execve
- Unit test: `process:try_lock` verifies CURRENT_PROCESS is accessible

**Failure mode:** If CURRENT_PROCESS is accidentally cleared, all subsequent syscalls would fail. This is detected by the selftest suite.

---

### INV-T4: Scheduler Never Deadlocks

**Statement:** `schedule()` and `try_schedule()` never deadlock. They use try_lock() for all locks in IRQ context.

**Boundary:** Timer tick, context switch

**Enforcement:**
- `tick()` uses try_lock() on scheduler
- `try_schedule()` uses try_lock() on scheduler
- `schedule()` uses lock() but is only called from process context (not IRQ)

**Verification:**
- Unit test: `scheduler:tick` verifies tick doesn't hang
- Code inspection: all IRQ-context paths use try_lock()

**Failure mode:** If someone adds a lock() call in the tick path, the system deadlocks. This is prevented by the coding convention: "no blocking locks in IRQ context."

---

### INV-T5: FD Table Independence

**Statement:** After fork, parent and child have independent fd tables. Modifying one does not affect the other.

**Boundary:** Fork, exec, close, dup

**Enforcement:**
- Fork clones fd table (deep copy)
- Each process has its own `files: IrqSafeMutex<ProcessFiles>`
- No shared references to fd tables between processes

**Verification:**
- Unit test: `process:fd_inherit` verifies clone independence
- Code inspection: fork path clones all three fd fields

**Failure mode:** If fd table is accidentally shared (e.g., by reference instead of clone), modifying one process's fds would corrupt the other's. This is prevented by Rust's ownership system.

---

## Soft Invariants (Preferences, Not Requirements)

### INV-T6: Emulation Mode Inheritance

**Statement:** When a process forks, the child inherits the parent's emulation mode.

**Boundary:** Fork, exec

**Enforcement:** `sys_clone` copies `parent.emulation` to child.

**Verification:** Unit test: `process:emulation_mode`

**Note:** This is a design decision, not a correctness requirement. The kernel could choose to reset emulation mode on fork.

### INV-T7: Process Table Registration

**Statement:** Every created process is registered in PROCESS_TABLE before it can be scheduled.

**Boundary:** Process creation, scheduling

**Enforcement:** `Process::register()` is called before `spawn_thread()`.

**Verification:** Unit test: `process:register_lookup`

**Note:** If a process is scheduled before registration, it would be invisible to wait4 and other process-management syscalls.
