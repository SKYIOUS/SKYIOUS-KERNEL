# Security Architecture

## Credential Model (UID/GID)

- Full POSIX credential set: `uid`, `gid`, `euid`, `egid`, `suid`, `sgid`, `fsuid`, `fsgid`
- Default: all 0 (root) for kernel-spawned processes
- Syscalls: `getuid`(301), `getgid`(302), `setuid`(303), `setgid`(304), `geteuid`(305), `getegid`(306)

### setuid rules

| Caller euid | Target uid | Result |
|-------------|------------|--------|
| 0 (root)    | any        | Sets uid + euid |
| matches target uid | same | Sets uid |
| otherwise | — | `EPERM` (requires `CAP_SETUID`) |

### DAC (Discretionary Access Control)

`check_file_permission(st_mode, st_uid, st_gid, need)` in `syscalls/mod.rs`:

- Root (euid=0) always passes
- If euid matches owner uid: checks owner permission bits (`mode >> 6 & 7`)
- If egid matches owner gid: checks group permission bits (`mode >> 3 & 7`)
- Otherwise: checks other permission bits (`mode & 7`)
- `need`: 4=read, 2=write, 1=execute

Checked in: `open`, `access`, `mkdir`, `unlink`, `execve`, `chdir`, `symlink`, `rename`, `mount`.

## Capabilities

Bitmask-based (each bit = one capability, Linux-compatible positions).

| Constant            | Bit | Use |
|---------------------|-----|-----|
| `CAP_CHOWN`         | 0   | — |
| `CAP_DAC_OVERRIDE`  | 1   | Bypass file permission checks |
| `CAP_KILL`          | 5   | `kill()` across UIDs |
| `CAP_SETUID`        | 6   | `setuid()` to any UID |
| `CAP_SETGID`        | 7   | `setgid()` to any GID |
| `CAP_SETPCAP`       | 8   | `capset()` (grant capabilities) |
| `CAP_NET_RAW`       | 13  | `socket(SOCK_RAW)` |
| `CAP_NET_ADMIN`     | 12  | Network administration |
| `CAP_SYS_ADMIN`     | 21  | `mount()`, privileged operations |
| `CAP_SYS_BOOT`      | 22  | `reboot()` |

### capset/capget

`SYS_CAPGET` (307) and `SYS_CAPSET` (308) use Linux v1 header format:

```c
struct cap_user_header { u32 version; pid_t pid; };
struct cap_user_data { u32 effective; u32 permitted; u32 inheritable; };
```

- capget: reads current process's capability sets
- capset: requires root (euid=0) **or** `CAP_SETPCAP` to modify

Both `cap_effective` and `cap_permitted` are writable. The kernel stores 64-bit masks internally but exposes only the lower 32 bits through this interface (sufficient for all defined capabilities).

### Gated syscalls

| Syscall | Guard |
|---------|-------|
| `mount` | euid==0 **or** `CAP_SYS_ADMIN` |
| `kill`  | euid==0 **or** same-uid **or** `CAP_KILL` + LSM |
| `socket(SOCK_RAW)` | `CAP_NET_RAW` |
| `setuid` | euid==0 **or** `CAP_SETUID` |
| `setgid` | euid==0 **or** `CAP_SETGID` |
| `capset` | euid==0 **or** `CAP_SETPCAP` |
| `reboot` | `CAP_SYS_BOOT` (checked in sys_reboot) |

## LSM (Linux Security Module)

Rule-based MAC system loaded from `/etc/lsm_policy` (see `security.rs`).

Format: `subject:object:class:perm:allow|deny`

Hooks alongside DAC in `open`, `mkdir`, `socket`, `kill`, `mount`, `execve`.

## Signals

- Per-process signal state: `pending` bitmask, `blocked` bitmask, 32 `signal_handlers` + `signal_restorers`
- `rt_sigaction` (13): register handler with optional restorer
- `rt_sigreturn` (15): restore saved context after handler
- `sigprocmask` (309): `SIG_BLOCK`(0), `SIG_UNBLOCK`(1), `SIG_SETMASK`(2)
- Delivery: after every syscall return in `do_syscall` postamble

### Signal-interruptible syscalls

| Syscall | Mechanism |
|---------|-----------|
| `nanosleep` | Timer tick wakes sleeping threads with pending signals |
| `accept`   | Pre-check at entry — returns EINTR |
| `read` (pipe) | Pre-check before blocking via `block_on_pipe` |

### Test programs

| Binary | Source | Tests |
|--------|--------|-------|
| `sigchld_test` | `tests/thread_test/src/sigchld_test.rs` | SIGCHLD handler via fork+wait |
| `sigint_test` | `tests/thread_test/src/sigint_test.rs` | SIGINT handler with nanosleep |
| `perm_test` | `tests/thread_test/src/perm_test.rs` | setuid + file permission DAC check |
| `futex_test` | `tests/thread_test/src/futex_test.rs` | Futex with signal interrupt |
