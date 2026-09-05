# Module Interface: `syscalls`

**Path:** `kernel/src/syscalls/`
**Owner:** TBD
**Tier:** 3 (depends on task, vfs, memory, net)

---

## Public API

### Dispatch (`syscalls/dispatch.rs`)

```rust
/// Main syscall dispatcher. Called from SYSCALL handler.
/// Routes to the appropriate handler based on syscall number.
pub fn do_syscall(n: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64, regs: *mut u64) -> u64

/// Linux syscall dispatcher. Called when process is in Linux emulation mode.
pub fn dispatch_linux_syscall(n: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64, regs: *mut u64) -> u64
```

### File I/O (`syscalls/fs_io.rs`)

```rust
pub fn sys_read(fd: u64, buf: *mut u8, count: usize) -> u64
pub fn sys_write(fd: u64, buf: *const u8, count: usize) -> u64
pub fn sys_open(path: *const u8, flags: i32, mode: u32) -> u64
pub fn sys_close(fd: u64) -> u64
pub fn sys_lseek(fd: u64, offset: i64, whence: i32) -> u64
pub fn sys_mmap(addr: u64, len: u64, prot: u64, flags: u64, fd: u64, offset: u64) -> u64
pub fn sys_munmap(addr: u64, len: u64) -> u64
pub fn sys_brk(addr: u64) -> u64
pub fn sys_mprotect(addr: u64, len: u64, prot: u64) -> u64
```

### Process (`syscalls/process_lifecycle.rs`)

```rust
pub fn sys_fork(regs_ptr: *mut u64) -> u64
pub fn sys_clone(flags: u64, child_stack: u64, parent_tid: *mut u32, child_tls: u64, child_tidptr: *mut u32, regs_ptr: *mut u64) -> u64
pub fn sys_execve(path_ptr: *const u8, argv_ptr: *const *const u8, envp_ptr: *const *const u8, regs_ptr: *mut u64) -> u64
pub fn sys_exit(status: u64) -> u64
pub fn sys_wait4(pid: i64, status_ptr: *mut i32, options: i32, rusage: *mut u8) -> u64
pub fn sys_getpid() -> u64
pub fn sys_getppid() -> u64
```

### Signal (`syscalls/process_signal.rs`)

```rust
pub fn sys_rt_sigaction(sig: u64, act: *const u64, oldact: *mut u64, sigsetsize: u64) -> u64
pub fn sys_rt_sigreturn(regs_ptr: *mut u64) -> u64
pub fn sys_kill(pid: i64, sig: u32) -> u64
pub fn sys_sigprocmask(how: i32, set_ptr: *const u64, oldset_ptr: *mut u64) -> u64
```

---

## Syscall Classification

| Status | Count | Meaning |
|--------|-------|---------|
| **[F]** Fully functional | ~40 | Works for all standard use cases |
| **[L]** Functional with limitations | ~30 | Works for common cases, has gaps |
| **[E]** Experimental | ~10 | Exists but untested |
| **[S]** Stub | ~15 | Returns hardcoded value |
| **[U]** Unsupported | ~35 | Not implemented |

**See `docs/syscall-classification.md` for the full list.**

---

## Invariants

1. **User pointer validation:** All user pointers must go through `copy_from_user`/`copy_to_user`. Never dereference userspace pointers directly.
2. **Error returns:** Syscalls return 0 on success, -errno on failure (Linux ABI). The error must be a valid errno value.
3. **FD table isolation:** Each process has its own fd table. Modifying one process's fds does not affect others.
4. **Exec atomicity:** exec replaces the address space atomically. If exec fails, the original address space is preserved.
5. **Signal safety:** Signal handlers run in userspace. The kernel only delivers signals; it does not execute handler code.

---

## Testing Requirements

| Test | What It Validates | Priority |
|------|-------------------|----------|
| **NEW: syscall:read_invalid_fd** | read with bad fd returns EBADF | ❌ Needed |
| **NEW: syscall:write_null_ptr** | write with null buf returns EFAULT | ❌ Needed |
| **NEW: syscall:exec_invalid_path** | exec with bad path returns ENOENT | ❌ Needed |
| **NEW: syscall:fork_exec_cycle** | fork → exec → exit → wait | ❌ Needed |
| **NEW: syscall:pipe_basic** | pipe creation and read/write | ❌ Needed |
| **NEW: syscall:mmap_anon** | Anonymous mmap works | ❌ Needed |

---

## Common Pitfalls

1. **Nested IrqSafeMutex in exec:** The fd table clone in sys_execve must use a single lock scope. The original bug was two separate lock() calls that could deadlock.
2. **CURRENT_PROCESS in exec:** After creating the new address space, CURRENT_PROCESS must be updated BEFORE jumping to userspace.
3. **Linux emulation:** Processes with EmulationMode::Linux are dispatched through `dispatch_linux_syscall`, which maps Linux syscall numbers to Vahi syscall numbers.
4. **User string reading:** Use `read_user_string()` for null-terminated strings. Use `copy_from_user()` for fixed-size buffers. Never trust userspace pointers.
