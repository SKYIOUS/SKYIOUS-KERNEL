# Vahi Kernel — Syscall Hygiene Audit

Generated from source inspection of `kernel/src/syscalls/` and `kernel/src/syscalls/user_access.rs`.

## Syscall Entry & Argument Flow (x86_64)

```
Userspace                          Kernel
─────────────────────────────────────────────────────────────────
syscall n, arg1..arg6 in RDI,RSI,RDX,R10,R8,R9
         │
         ▼
syscall_entry (asm, kernel/src/syscalls/mod.rs)
  push r11, rcx, rax, rcx, rdx, rbx, rbp, rsi, rdi, r8, r9,
        r10, r11, r12, r13, r14, r15
         │
         ▼
do_syscall(n, arg1..arg5, regs_ptr) in dispatch.rs:1516
  1. ash_syscall_denied()  ──► EPERM if denied
  2. seccomp::check_syscall() ──► EPERM if denied
  3. do_syscall_body()
       SyscallArgs { n, a1..a5, regs: regs_ptr }
         │
         ▼
    SYSCALL_TABLE[n](&args)  ──► handler(args)
         │
         ▼
    handler returns u64 (errno or value)
```

### SyscallArgs Structure (dispatch.rs:142)
```rust
pub struct SyscallArgs {
    pub n: u64,                    // syscall number
    pub a1: u64,  pub a2: u64,     // arg1..arg5 from userspace
    pub a3: u64,  pub a4: u64,
    pub a5: u64,
    pub regs: *mut u64,            // pointer to saved user register frame
}
```

### sys_handler! Macro (dispatch.rs:159)
```rust
// All three forms generate a closure: &SyscallArgs -> u64
sys_handler!(table, args, NUM, family::fn(arg1, arg2, ...))
```
Arguments are extracted from `args.a1..a5` at the call site.

---

## Userspace Pointer Validation (user_access.rs)

### Core Validation
```rust
pub fn validate_ptr(ptr: *const u8, len: usize) -> bool {
    let end = ptr as u64 + len as u64;
    end <= 0x0000_8000_0000_0000  // userspace limit
}
```

### Copy Helpers
```rust
unsafe fn copy_from_user(dst: &mut [u8], src: *const u8) -> Result<(), ()>
unsafe fn copy_to_user(dst: *mut u8, src: &[u8]) -> Result<(), ()>
unsafe fn read_user_string(ptr: *const u8, max_len: usize) -> Result<String, ()>
unsafe fn copy_struct_to_user<T: Copy>(dst: *mut u8, src: &T) -> Result<(), ()>
```

### Validation Gaps Identified

| Gap | Location | Risk |
|-----|----------|------|
| No pointer alignment check | `validate_ptr` | Unaligned access may fault or leak data |
| No NULL pointer differentiation | `validate_ptr` accepts null | NULL passes validation; `copy_from_user` handles it but inconsistent |
| No overlapping range check | `validate_ptr` | Overlapping user/kernel ranges not detected |
| No `len == 0` special case | `validate_ptr` | Zero-length passes; copy does nothing |
| No `is_null()` check before `copy_to_user` | `copy_to_user` | Null destination returns `Err(())` but no audit log |

---

## Syscall Dispatch Table Hygiene

### Current State (219 declared, 199 dispatched)

| Category | Count | Notes |
|----------|-------|-------|
| Declared in numbers.rs | 219 | |
| Dispatched in dispatch.rs | 199 | 10 truly missing (ASH, epoll native, VM) |
| Inline dispatch patterns | ~20 | Uses inline `fs::sys_xxx()` calls |

### Missing Dispatch (10 syscalls)

| Syscall | Number | Category |
|---------|--------|----------|
| ASH_REGISTER | 310 | ASH scripting |
| ASH_UNREGISTER | 311 | ASH scripting |
| ASH_STATS | 312 | ASH scripting |
| ASH_CONTROL | 313 | ASH scripting |
| EPOLL_CREATE1_NATIVE | 450 | Vahi-native epoll |
| EPOLL_CTL_NATIVE | 451 | Vahi-native epoll |
| EPOLL_WAIT_NATIVE | 452 | Vahi-native epoll |
| SYSINFO | 205 | Comment: "moved from 203" |
| VM_CREATE | 340 | Hypervisor |
| VM_DESTROY | 341 | Hypervisor |
| VM_GET_INFO | 347 | Hypervisor |
| VM_INJECT_IRQ | 349 | Hypervisor |
| VM_LOAD_KERNEL | 346 | Hypervisor |
| VM_PAUSE | 344 | Hypervisor |
| VM_RESUME | 345 | Hypervisor |
| VM_SET_MEMORY | 348 | Hypervisor |
| VM_START | 342 | Hypervisor |
| VM_STOP | 343 | Hypervisor |

**Note**: BRK, CLOSE, DUP, DUP2, EXIT, FORK, FUTEX, GETEGID, GETEUID, GETGID, GETPGID, GETPGRP, GETPID, GETPPID, GETSID, GETUID, MMAP, OPENPTY, PAUSE, PPOLL, PR_LIMIT64, PSELECT6, RECVFROM, SENDTO, SETGID, SETSID, SETUID, SYNC, SYSINFO, UMASK — these use inline dispatch patterns (`fs::sys_xxx()`, `process::sys_xxx()`) and ARE implemented but not caught by `numbers::SYS_` grep.

---

## Argument Validation Patterns in Dispatch

### Current Patterns (Inconsistent)

| Syscall | Args Extracted | Validation |
|---------|----------------|------------|
| `mq_open` | `args.a1 as *const u8, args.a2 as i32, args.a3 as i32, args.a4 as *mut u8` | `read_name` validates name; `copy_mq_attr_from_user` validates attr |
| `mq_timedsend` | `args.a1 as i32, args.a2 as *const u8, args.a3 as usize, args.a4, args.a5 as *const u8` | `read_deadline` validates timeout; `copy_from_user` validates msg |
| `mq_timedreceive` | `args.a1 as i32, args.a2 as *mut u8, args.a3 as usize, args.a4 as *mut u32, args.a5 as *const u8` | `read_deadline`; `copy_to_user` validates msg buffer |
| `mq_notify` | `args.a1 as i32, args.a2 as *const u8` | `read_sigevent` validates event |
| `mq_unlink` | `args.a1 as *const u8` | `read_name` validates name |
| `mq_getsetattr` | `args.a1 as i32, args.a2 as *const u8, args.a3 as *mut u8` | `copy_mq_attr_from_user`/`to_user` validate attr |

### Missing Validation Patterns

| Missing Check | Syscalls Affected | Risk |
|---------------|-------------------|------|
| File descriptor range check | All fd-based syscalls | EBADF vs kernel panic |
| File descriptor type check | `read`/`write`/`close`/`ioctl`/`fcntl` | Wrong fd type corrupts state |
| Buffer size vs `count` check | `read`/`write`/`recvmsg`/`sendmsg` | Buffer overflow / info leak |
| `iovec` validation | `readv`/`writev`/`recvmmsg`/`sendmmsg` | Kernel memory corruption |
| `iovec` count limit | `readv`/`writev`/`recvmmsg`/`sendmmsg` | Excessive kernel stack allocation |
| `sigevent` field validation | `mq_notify`/`timer_create`/`signalfd4` | Invalid signal delivery |
| `timespec` normalization | `timer_*`, `clock_*`, `nanosleep` | Time warp / overflow |
| `rlimit` resource index | `getrlimit`/`setrlimit`/`prlimit64` | Array OOB |
| `capability` bit range | `capget`/`capset`/`prctl` | Invalid capability |
| `prctl` option validation | `prctl` | Invalid option crashes kernel |
| `prlimit64` resource bounds | `prlimit64` | Invalid resource index |
| `epoll` fd validation | `epoll_ctl`/`epoll_wait`/`epoll_pwait` | Invalid epfd |
| `timerfd` flags | `timerfd_create`/`timerfd_settime` | Invalid flags |
| `signalfd` mask validation | `signalfd`/`signalfd4` | Invalid signal mask |
| `eventfd` initval | `eventfd`/`eventfd2` | Overflow |
| `inotify` watch mask | `inotify_add_watch`/`inotify_rm_watch` | Invalid mask |
| `fanotify` mask/flags | `fanotify_init`/`fanotify_mark` | Invalid mask |
| `bpf` cmd/map validation | `bpf` | BPF VM crash |
| `seccomp` filter validation | `seccomp` | Filter bypass |
| `landlock` ruleset validation | `landlock_*` | Sandbox escape |
| `io_uring` params | `io_uring_setup`/`enter`/`register` | Ring buffer corruption |
| `pidfd` validation | `pidfd_open`/`pidfd_getfd`/`pidfd_send_signal` | PID reuse race |

---

## Error Number (errno) Mapping Audit

### Current errno.rs (from kernel/src/syscalls/errno.rs)

| Vahi Errno | Linux Value | Status |
|------------|-------------|--------|
| EPERM | 1 | ✓ |
| ENOENT | 2 | ✓ |
| ESRCH | 3 | ✓ |
| EINTR | 4 | ✓ |
| EIO | 5 | ✓ |
| ENXIO | 6 | ✓ |
| E2BIG | 7 | ✓ |
| ENOEXEC | 8 | ✓ |
| EBADF | 9 | ✓ |
| ECHILD | 10 | ✓ |
| EAGAIN | 11 | ✓ |
| ENOMEM | 12 | ✓ |
| EACCES | 13 | ✓ |
| EFAULT | 14 | ✓ |
| ENOTBLK | 15 | ✓ |
| EBUSY | 16 | ✓ |
| EEXIST | 17 | ✓ |
| EXDEV | 18 | ✓ |
| ENODEV | 19 | ✓ |
| ENOTDIR | 20 | ✓ |
| EISDIR | 21 | ✓ |
| EINVAL | 22 | ✓ |
| ENFILE | 23 | ✓ |
| EMFILE | 24 | ✓ |
| ENOTTY | 25 | ✓ |
| ETXTBSY | 26 | ✓ |
| EFBIG | 27 | ✓ |
| ENOSPC | 28 | ✓ |
| ESPIPE | 29 | ✓ |
| EROFS | 30 | ✓ |
| EMLINK | 31 | ✓ |
| EPIPE | 32 | ✓ |
| EDOM | 33 | ✓ |
| ERANGE | 34 | ✓ |
| EDEADLK | 35 | ✓ |
| ENAMETOOLONG | 36 | ✓ |
| ENOLCK | 37 | ✓ |
| ENOSYS | 38 | ✓ |
| ENOTEMPTY | 39 | ✓ |
| ELOOP | 40 | ✓ |
| ENOMSG | 42 | ✓ |
| EIDRM | 43 | ✓ |
| ECHRNG | 44 | ? |
| EL2NSYNC | 45 | ? |
| EL3HLT | 46 | ? |
| EL3RST | 47 | ? |
| ELNRNG | 48 | ? |
| EUNATCH | 49 | ? |
| ENOCSI | 50 | ? |
| EL2HLT | 51 | ? |
| EBADE | 52 | ? |
| EBADR | 53 | ? |
| EXFULL | 54 | ? |
| ENOANO | 55 | ? |
| EBADRQC | 56 | ? |
| EBADSLT | 57 | ? |
| EDEADLOCK | 58 | = EDEADLK? |
| EBFONT | 59 | ? |
| ENOSTR | 60 | ? |
| ENODATA | 61 | ? |
| ETIME | 62 | ? |
| ENOSR | 63 | ? |
| ENONET | 64 | ? |
| ENOPKG | 65 | ? |
| EREMOTE | 66 | ? |
| ENOLINK | 67 | ? |
| EADV | 68 | ? |
| ESRMNT | 68 | ? |
| ECOMM | 70 | ? |
| EPROTO | 71 | ? |
| EMULTIHOP | 72 | ? |
| EDOTDOT | 73 | ? |
| EBADMSG | 74 | ? |
| EOVERFLOW | 75 | ✓ |
| ENOTUNIQ | 76 | ? |
| EBADFD | 77 | ? |
| EREMCHG | 78 | ? |
| ELIBACC | 79 | ? |
| ELIBBAD | 80 | ? |
| ELIBSCN | 81 | ? |
| ELIBMAX | 82 | ? |
| ELIBEXEC | 83 | ? |
| EILSEQ | 84 | ✓ |
| ERESTART | 85 | ? |
| ESTRPIPE | 86 | ? |
| EUSERS | 87 | ? |
| ENOTSOCK | 88 | ✓ |
| EDESTADDRREQ | 89 | ✓ |
| EMSGSIZE | 90 | ✓ |
| EPROTOTYPE | 91 | ✓ |
| ENOPROTOOPT | 92 | ✓ |
| EPROTONOSUPPORT | 93 | ✓ |
| ESOCKTNOSUPPORT | 94 | ✓ |
| EOPNOTSUPP | 95 | ✓ |
| EPFNOSUPPORT | 96 | ✓ |
| EAFNOSUPPORT | 97 | ✓ |
| EADDRINUSE | 98 | ✓ |
| EADDRNOTAVAIL | 99 | ✓ |
| ENETDOWN | 100 | ✓ |
| ENETUNREACH | 101 | ✓ |
| ENETRESET | 102 | ✓ |
| ECONNABORTED | 103 | ✓ |
| ECONNRESET | 104 | ✓ |
| ENOBUFS | 105 | ✓ |
| EISCONN | 106 | ✓ |
| ENOTCONN | 107 | ✓ |
| ESHUTDOWN | 108 | ✓ |
| ETOOMANYREFS | 109 | ✓ |
| ETIMEDOUT | 110 | ✓ |
| ECONNREFUSED | 111 | ✓ |
| EHOSTDOWN | 112 | ✓ |
| EHOSTUNREACH | 113 | ✓ |
| EALREADY | 114 | ✓ |
| EINPROGRESS | 115 | ✓ |
| ESTALE | 116 | ✓ |
| EUCLEAN | 117 | ? |
| ENOTNAM | 118 | ? |
| ENAVAIL | 119 | ? |
| EISNAM | 120 | ? |
| EREMOTEIO | 121 | ✓ |
| EDQUOT | 122 | ✓ |
| ENOMEDIUM | 123 | ✓ |
| EMEDIUMTYPE | 124 | ✓ |
| ECANCELED | 125 | ✓ |
| ENOKEY | 126 | ? |
| EKEYEXPIRED | 127 | ? |
| EKEYREJECTED | 128 | ? |
| EOWNERDEAD | 130 | ? |
| ENOTRECOVERABLE | 131 | ? |
| ERFKILL | 131 | ? |

### Missing/Unverified Errnos (marked ?)

Need verification against Linux x86_64 `asm-generic/errno-base.h` and `errno.h`.

---

## Seccomp Integration (seccomp.rs)

### Current State
- BPF filter evaluation on syscall entry
- `check_syscall(n, args)` called before dispatch
- Returns `Some(errno)` to deny, `None` to allow

### Gaps
- No filter installation syscall (`seccomp` syscall at 317) fully implemented
- No `SECCOMP_FILTER_FLAG_TSYNC` support
- No `SECCOMP_FILTER_FLAG_LOG` support
- No `SECCOMP_RET_LOG` action
- No `SECCOMP_RET_KILL_PROCESS` action
- No notification mechanism (`SECCOMP_RET_USER_NOTIF`)

---

## Recommended Hygiene Fixes (Priority Order)

### P0: Critical (Security/Correctness)
1. **Add fd validation helper** — `validate_fd(fd, need_read, need_write) -> Result<FileDescriptor, Errno>`
2. **Add buffer validation** — `validate_user_buffer(ptr, len, writable) -> Result<(), Errno>`
3. **Add iovec validation** — `validate_iovec(iov, count) -> Result<usize, Errno>` (returns total bytes)
4. **Add timespec normalization** — `normalize_timespec(ts) -> Result<u64_ns, Errno>`
4. **Add signal number validation** — `validate_signo(signo) -> Result<(), Errno>`
5. **Add capability validation** — `validate_capability(cap) -> Result<(), Errno>`
6. **Add errno verification** — Verify all `?` entries in errno.rs against Linux

### P1: High (Correctness)
7. **Dispatch table completeness** — Implement missing 10 dispatches (ASH, epoll native, VM)
8. **Inline dispatch refactor** — Move inline `fs::sys_xxx()` calls to `sys_handler!` macro
9. **Add fd type check** — `check_fd_type(fd, expected_type) -> Result<(), Errno>`
10. **Add buffer size checks** — `read`/`write`/`sendmsg`/`recvmsg` must check `len <= buffer_size`

### P2: Medium (Hygiene)
11. **Standardize arg extraction** — Use `args.arg1()` accessor methods instead of raw `args.a1`
12. **Add struct copy helpers** — `copy_struct_from_user<T>`, `copy_struct_to_user<T>`
13. **Add pointer alignment check** — `validate_aligned(ptr, align)`
14. **Add overlapping range check** — `validate_nonoverlapping(ptr1, len1, ptr2, len2)`
15. **Add seccomp filter install** — Implement `seccomp` syscall fully
16. **Verify errno.rs** — Match all Linux x86_64 errno values exactly

### P3: Low (Maintainability)
17. **Document syscall conventions** — Update `docs/interfaces/syscalls.md`
18. **Add negative-path tests** — One test per syscall for bad pointers, invalid FDs, EINVAL
19. **Add fuzzer corpus** — Extend `tests/fuzzer.rs` with syscall argument fuzzing

---

## Verification Checklist

| Check | Command | Status |
|-------|---------|--------|
| Build release | `cargo build --release --target x86_64-unknown-none --features "self_test,smp,net,ext4,iommu" --locked` | ✅ |
| Clippy release | `cargo clippy --release --target x86_64-unknown-none --features "self_test,smp,net,ext4,iommu" --locked -- -D warnings` | ✅ |
| Host tests | `cargo test --workspace --exclude vahi_kernel --all-features --locked` | ✅ |
| Host clippy | `cargo clippy --workspace --exclude vahi_kernel --all-targets --all-features --locked -- -D warnings` | ✅ |
| Policy self-test | `py scripts/check_repo_policy.py --self-test` | ✅ |
| Policy worktree | `py scripts/check_repo_policy.py --worktree` | ✅ |
| Format check | `cargo fmt --all -- --check` | ✅ |
| QEMU self-test (release) | `py tests/run_qemu_tests.py --image ... --expect-count 228` | ✅ (228/228) |
| QEMU self-test (audit_small) | `py tests/run_qemu_tests.py --image ... --expect-count 228` | ✅ (228/228) |

---

## Next Steps (Recommended Order)

1. **Implement missing dispatches** (10 syscalls) — unblock hypervisor/ASH features
2. **Add validation helpers** (`validate_fd`, `validate_user_buffer`, `validate_iovec`) — reduce bug surface
3. **Refactor inline dispatches** — Use `sys_handler!` macro consistently
4. **Add negative-path tests** — One per syscall for bad args
5. **Verify errno.rs** against Linux kernel source

---

*Document generated from source inspection. Run `cargo build --release --target x86_64-unknown-none --features "self_test,smp,net,ext4,iommu" --locked` and QEMU self-test to verify changes.*