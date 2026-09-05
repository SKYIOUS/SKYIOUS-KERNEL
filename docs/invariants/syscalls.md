# Invariants: `syscalls` Module

**Last verified:** 2026-08-29
**Verified by:** Invariant-driven design analysis

---

## Critical Invariants

### INV-S1: User Pointer Validation

**Statement:** All user pointers must go through `copy_from_user`/`copy_to_user`. Never dereference userspace pointers directly.

**Boundary:** All syscall handlers

**Enforcement:**
- `user_access::copy_from_user()` validates pointer range before reading
- `user_access::copy_to_user()` validates pointer range before writing
- `user_access::read_user_string()` validates null-terminated string

**Verification:**
- Code inspection: all syscall handlers use copy_from_user/copy_to_user
- No direct dereference of userspace pointers

**Failure mode:** Direct dereference of userspace pointers could read/write kernel memory, causing data corruption or security vulnerabilities.

---

### INV-S2: Error Returns

**Statement:** Syscalls return 0 on success, -errno on failure (Linux ABI). The error must be a valid errno value.

**Boundary:** All syscall handlers

**Enforcement:**
- Each syscall returns `errno::Errno::XXX as u64` on failure
- Success returns 0 or a valid result value

**Verification:**
- Code inspection: all syscall handlers return errno values
- No silent failures (every error path returns an errno)

**Failure mode:** Returning 0 on failure would make the caller think the operation succeeded. Returning an invalid errno would confuse error handling.

---

### INV-S3: FD Table Isolation

**Statement:** Each process has its own fd table. Modifying one process's fds does not affect others.

**Boundary:** open, close, dup, dup2, exec

**Enforcement:**
- Each process has `files: IrqSafeMutex<ProcessFiles>`
- Fork clones the fd table (deep copy)
- No shared references between processes

**Verification:**
- Unit test: `process:fd_inherit` verifies clone independence
- Code inspection: no shared fd table references

**Failure mode:** Shared fd table references would cause one process's close/dup to affect another process's file descriptors.

---

### INV-S4: Exec Atomicity

**Statement:** exec replaces the address space atomically. If exec fails, the original address space is preserved.

**Boundary:** execve syscall

**Enforcement:**
- New address space is created BEFORE the old one is destroyed
- If any step fails, the function returns an error without modifying the process
- CURRENT_PROCESS is updated only after successful ELF load

**Verification:**
- Code inspection: exec path creates new AddressSpace, then replaces old
- No partial state visible on failure

**Failure mode:** If exec partially modifies the process state before failing, the process would be in an inconsistent state (half-old, half-new address space).

---

### INV-S5: Signal Safety

**Statement:** Signal handlers run in userspace. The kernel only delivers signals; it does not execute handler code.

**Boundary:** Signal delivery, signal return

**Enforcement:**
- `sys_rt_sigaction` stores the handler address
- Signal delivery sets up the user stack with the handler frame
- `sys_rt_sigreturn` restores the original context

**Verification:**
- Code inspection: signal delivery pushes a frame to user stack
- No kernel code executes signal handlers

**Failure mode:** If the kernel tried to execute signal handler code, it would run with kernel privileges, bypassing all security boundaries.

---

### INV-S6: No Nested IrqSafeMutex in Exec

**Statement:** sys_execve must not hold CURRENT_PROCESS.lock() while calling another function that locks CURRENT_PROCESS.

**Boundary:** execve syscall

**Enforcement:**
- fd table clone happens at the very start, under a single lock scope
- Lock is dropped before any heavy work (ELF load, address space creation)
- CURRENT_PROCESS is re-acquired only at the end to update the pointer

**Verification:**
- Code inspection: single lock scope for fd clone
- Unit test: `process:try_lock` verifies CURRENT_PROCESS is accessible

**Failure mode:** Nested IrqSafeMutex locks cause deadlock (infinite spin with interrupts disabled).

---

## Lock Ordering (Within Syscalls Module)

```
CURRENT_PROCESS → process.files → REFCOUNTS → BUDDY_ALLOCATOR
```

**This ordering must be respected in all syscall handlers.**

**Verification:**
- Code inspection: all syscall handlers follow this ordering
- The exec fd clone fix was specifically to enforce this ordering

**Failure mode:** Reversing the ordering causes ABBA deadlock.
