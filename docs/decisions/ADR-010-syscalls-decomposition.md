# ADR-010: Decompose the Syscall Layer

## Status
Proposed

## Date
2026-08-20

## Context
`kernel/src/syscalls/mod.rs` is 7,301 lines — the largest file in the kernel (selftest gate runs TAP 92/92, so it works, but it is a god file: dispatch + every handler + user_access helpers in one module). The architecture review flagged it as the #1 file. `syscalls/numbers.rs` (216 lines) already shows the intended shape: one file per concern.

## Decision
Split `syscalls/mod.rs` into per-domain files, keeping `mod.rs` as the thin dispatch table only (the `match` on syscall numbers):

```
syscalls/
  mod.rs            # dispatch match (thin), re-exports
  numbers.rs        # syscall numbers (unchanged)
  user_access.rs    # existing copy_to/from_user helpers (moved out if inline)
  fs.rs             # sys_open/read/write/close/seek/stat/getdents64/mkdir/...
  process.rs        # sys_fork/execve/exit/wait/kill/sched_*/...
  net.rs            # sys_socket/connect/send/recv/... (cfg(feature = "net"))
  ipc.rs            # sys_pipe/dup/futex/mq/...
  gui.rs            # sys_gui_* (cfg(feature = "gpu"))
  misc.rs           # sys_uname/sysinfo/clock/gettimeofday/...
  timers.rs         # posix_timers (already separate: syscalls/posix_timers.rs)
```

Dispatch arms stay in `mod.rs`; each handler `fn sys_*` moves to its domain file and is imported. No syscall numbers change — the ABI is frozen (docs/SYSCALL_ABI.md).

## Alternatives Considered

### Leave as one file
- Pros: zero churn, dispatch is one hop
- Cons: 7,300-line file is unmaintainable; every diff touches the same file → merge friction; the review flagged it
- Rejected

### Trait-based dispatch (Handler trait per domain)
- Pros: extensible, testable
- Cons: abstraction with one implementation; adds a layer for no measurable gain in a monolithic kernel (ADR-001)
- Rejected: flat `fn` per domain is the boring, correct shape

## Consequences
- Numbers, ABI, and behavior unchanged — split is mechanical (move + import)
- Each domain file compiles independently under its feature gate (net.rs only with `net`)
- Enables per-domain review: process.rs is the security-sensitive one
- `syscalls/mod.rs` shrinks from 7,301 lines to roughly the dispatch match + domain imports (~200 lines)