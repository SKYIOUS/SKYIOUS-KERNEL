# SkyOS Syscall ABI

This document specifies the Application Binary Interface (ABI) for making system calls to the Vahi kernel.

**Status: FROZEN for v1.0** — syscall numbers will not change after v1.0-rc1.

## 1. Invocation

- **Instruction:** `syscall`
- **Register Usage:**
  - `rax`: Syscall number
  - `rdi`: Argument 1
  - `rsi`: Argument 2
  - `rdx`: Argument 3
  - `r10`: Argument 4
  - `r8`:  Argument 5
  - `r9`:  Argument 6
- **Return Value:** The result of the syscall is returned in `rax`.
- **Clobbered Registers:** `rcx` and `r11` are clobbered by the `syscall`/`sysret` instructions.

## 2. Return Values and Errors

- **Success:** A non-negative value in `rax` indicates success.
- **Error:** A negative value in `rax` indicates an error (`-errno`).

## 3. Syscall Numbers (Frozen)

Syscall numbers follow Linux x86_64 convention where applicable.

### File I/O
| # | Name | arg1 | arg2 | arg3 | arg4 | arg5 |
|---|------|------|------|------|------|------|
| 0 | `read` | fd: i32 | buf: *mut u8 | count: usize | | |
| 1 | `write` | fd: i32 | buf: *const u8 | count: usize | | |
| 2 | `open` | path: *const u8 | flags: i32 | | | |
| 3 | `close` | fd: i32 | | | | |
| 4 | `stat` | path: *const u8 | buf: *mut Stat | | | |
| 5 | `fstat` | fd: i32 | buf: *mut Stat | | | |
| 8 | `lseek` | fd: i32 | offset: i64 | whence: i32 | | |
| 16 | `ioctl` | fd: i32 | request: u64 | arg: *mut u8 | | |
| 21 | `access` | path: *const u8 | mode: i32 | | | |
| 22 | `pipe` | fds: *mut i32 | | | | |
| 23 | `select` | nfds: i32 | readfds: *mut fd_set | writefds: *mut fd_set | exceptfds: *mut fd_set | timeout: *const timeval |
| 32 | `dup` | fd: i32 | | | | |
| 33 | `dup2` | oldfd: i32 | newfd: i32 | | | |
| 72 | `fcntl` | fd: i32 | cmd: i32 | arg: u64 | | |
| 79 | `getcwd` | buf: *mut u8 | size: usize | | | |
| 80 | `chdir` | path: *const u8 | | | | |
| 82 | `rename` | oldpath: *const u8 | newpath: *const u8 | | | |
| 83 | `mkdir` | path: *const u8 | mode: u32 | | | |
| 87 | `unlink` | path: *const u8 | | | | |
| 88 | `symlink` | target: *const u8 | linkpath: *const u8 | | | |
| 89 | `readlink` | path: *const u8 | buf: *mut u8 | size: usize | | |
| 91 | `fchmod` | fd: i32 | mode: u32 | | | |
| 93 | `fchown` | fd: i32 | owner: u32 | group: u32 | | |
| 137 | `statfs` | path: *const u8 | buf: *mut StatFs | | | |
| 165 | `mount` | source: *const u8 | target: *const u8 | fstype: *const u8 | flags: u64 | data: *const u8 |
| 167 | `umount2` | target: *const u8 | flags: u64 | | | |
| 217 | `getdents64` | fd: i32 | dirp: *mut u8 | count: usize | | |

### Memory
| # | Name | arg1 | arg2 | arg3 | arg4 | arg5 |
|---|------|------|------|------|------|------|
| 9 | `mmap` | addr: u64 | len: usize | prot: i32 | flags: i32 | fd: i32 |
| 11 | `munmap` | addr: u64 | len: usize | | | |
| 12 | `brk` | addr: u64 | | | | |

### Process
| # | Name | arg1 | arg2 | arg3 | arg4 | arg5 |
|---|------|------|------|------|------|------|
| 39 | `getpid` | | | | | |
| 56 | `clone` | flags: u64 | child_stack: u64 | parent_tid: *mut u32 | child_tls: u64 | child_tidptr: *mut u32 |
| 57 | `fork` | | | | | |
| 59 | `execve` | path: *const u8 | argv: *const *const u8 | envp: *const *const u8 | | |
| 60 | `exit` | status: i32 | | | | |
| 61 | `wait4` | pid: i64 | status: *mut i32 | options: i32 | rusage: *mut u8 | |
| 110 | `getppid` | | | | | |
| 144 | `sched_setattr` | pid: i64 | attr: *const u8 | flags: u64 | | |
| 145 | `sched_getattr` | pid: i64 | attr: *mut u8 | size: u32 | flags: u64 | |
| 158 | `arch_prctl` | code: u64 | addr: u64 | | | |
| 218 | `set_tid_address` | tidptr: *mut u32 | | | | |
| 231 | `exit_group` | status: i32 | | | | |
| 301 | `getuid` | | | | | |
| 302 | `getgid` | | | | | |
| 303 | `setuid` | uid: u32 | | | | |
| 304 | `setgid` | gid: u32 | | | | |
| 305 | `geteuid` | | | | | |
| 306 | `getegid` | | | | | |

### Signals
| # | Name | arg1 | arg2 | arg3 | arg4 |
|---|------|------|------|------|------|
| 13 | `rt_sigaction` | sig: i32 | act: *const u64 | oldact: *mut u64 | |
| 15 | `rt_sigreturn` | (implicit from stack) | | | |
| 62 | `kill` | pid: i64 | sig: u32 | | |

### Networking
| # | Name | arg1 | arg2 | arg3 | arg4 | arg5 |
|---|------|------|------|------|------|------|
| 41 | `socket` | domain: i32 | type: i32 | protocol: i32 | | |
| 42 | `connect` | fd: i32 | addr: *const u8 | addrlen: i32 | | |
| 43 | `accept` | fd: i32 | addr: *mut u8 | addrlen: *mut u32 | | |
| 44 | `sendto` | fd: i32 | buf: *const u8 | len: i32 | flags: i32 | dest: *const u8 |
| 45 | `recvfrom` | fd: i32 | buf: *mut u8 | len: i32 | flags: i32 | addr: *mut u8 |
| 49 | `bind` | fd: i32 | addr: *const u8 | addrlen: i32 | | |
| 50 | `listen` | fd: i32 | backlog: i32 | | | |
| 200 | `resolve` | host: *const u8 | addr: *mut u8 | | | |

### Timing
| # | Name | arg1 | arg2 | arg3 |
|---|------|------|------|------|
| 24 | `sched_yield` | | | |
| 35 | `nanosleep` | req: *const Timespec | rem: *mut Timespec | |
| 228 | `clock_gettime` | clock_id: i32 | tp: *mut Timespec | |

### Synchronization
| # | Name | arg1 | arg2 | arg3 |
|---|------|------|------|------|
| 202 | `futex` | uaddr: *mut u32 | op: u32 | val: u32 |

### GUI
| # | Name | arg1 | arg2 | arg3 | arg4 | arg5 |
|---|------|------|------|------|------|------|
| 100 | `gui_create_window` | title: *const u8 | width: usize | height: usize | | |
| 101 | `gui_get_buffer` | window_id: u64 | | | | |
| 102 | `gui_flush` | window_id: u64 | region: *const u32 | | | |
| 103 | `gui_map_buffer` | window_id: u64 | | | | |
| 105 | `gui_get_key` | window_id: u64 | | | | |
| 120 | `gui_get_mouse` | window_id: u64 | | | | |
| 121 | `gui_set_title` | window_id: u64 | title: *const u8 | | | |
| 122 | `gui_destroy_window` | window_id: u64 | | | | |
| 123 | `gui_resize_window` | window_id: u64 | width: usize | height: usize | | |
| 124 | `gui_move_window` | window_id: u64 | x: i32 | y: i32 | | |
| 125 | `clipboard` | op: u32 | buf: *mut u8 | len: u32 | | |
| 126 | `notify` | message: *const u8 | duration_ms: u32 | | | |

### Kernel / Misc
| # | Name | arg1 | arg2 | arg3 |
|---|------|------|------|------|
| 36 | `sync` | | | |
| 63 | `uname` | buf: *mut UtsName | | |
| 104 | `beep` | freq: u32 | duration_ms: u32 | |
| 169 | `reboot` | magic1: u32 | magic2: u32 | cmd: u32 |
| 203 | `sysinfo` | info: *mut u64 | | |
| 300 | `vahiai` | input: *const u8 | args: *const *const u8 | arg_count: u32 | output: *mut u8 | max_output: u32 |
| 321 | `bpf` | cmd: i32 | attr: *mut u8 | size: u32 | |
| 400 | `drmctl` | cmd: u32 | arg: u64 | data: *mut u8 | |
| 401 | `hash` | algo: u32 | data: *const u8 | len: u32 | out: *mut u8 | out_len: u32 |
| 425 | `io_uring_setup` | entries: u32 | | | |
| 426 | `io_uring_enter` | ring_fd: u32 | to_submit: u32 | min_complete: u32 | flags: u32 | sig: *mut u8 |

## 4. libsarga ABI

- Binary format: ELF64 x86-64, PIE (`ET_DYN`)
- Crate type: `rlib` (static) via `libsarga.rlib`
- All userspace programs link against `libsarga` as their runtime
- Entry point: `_start` at `0x400000` (PIE base)
- `_start` receives `argc` in `rdi`, `argv` in `rsi`, `envp` in `rdx`
- Stack: 16-byte aligned, guard pages below each stack allocation
- Heap: managed via `brk` syscall, libsarga's `mem.rs` wraps it

## 5. Threading ABI

- Thread creation: `clone` syscall with `CLONE_VM | CLONE_SETTLS | CLONE_CHILD_CLEARTID`
- TLS: FS base set via `CLONE_SETTLS` in clone, or `arch_prctl(ARCH_SET_FS, addr)`
- FS base is **saved/restored on context switch** (kernel does this)
- Thread exit: `clear_child_tid` pointer is zeroed and futex-woken
- Join: wait on `clear_child_tid` via futex `FUTEX_WAIT`
- `Mutex`: atomic exchange + futex wait/wake
- `Condvar`: futex-based (FUTEX_WAIT + FUTEX_REQUEUE in future)

## 6. Signal Handling

- Signal handlers set via `rt_sigaction`
- Signal frame pushed onto user stack before handler runs
- `rt_sigreturn` restores context from signal frame
- Supported signals: SIGTERM(15), SIGKILL(9), SIGCHLD(17), etc.
