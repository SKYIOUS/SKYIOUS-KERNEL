# vahi-task

Process and thread management — lifecycle, scheduling, OOM killer, context switching.

## Modules

| Module | Lines | Contents |
|--------|-------|----------|
| `process` | 1,034 | Process struct, fork, exec, exit, wait, signals |
| `thread` | 762 | Thread struct, context switch, FPU save/restore |
| `scheduler` | 1,165 | Stride heap, work stealing, CPU affinity |
| `oom` | 528 | OOM killer (age/root scoring) |

## Key Types

| Type | Defined In | Purpose |
|------|-----------|---------|
| `ProcessState` | lib.rs | Running/Sleeping/Stopped/Zombie |
| `CloneFlags` | lib.rs | Linux-compatible CLONE_VM, CLONE_FILES, etc. |
| `Signal` | lib.rs | All 31 Linux signals with default actions |
| `SchedPolicy` | lib.rs | Normal/Batch/Idle scheduling |
| `ThreadContext` | thread.rs | Saved registers for context switch |
| `FpuState` | thread.rs | XSAVE area (512+ bytes, 64-byte aligned) |

## Dependency Breaking

```text
Original:     task → memory (AddressSpace) + vfs (FileDescriptor)
With traits:  task → vahi_types::{Vma, FileDescriptor, ProcessProvider}
```

## Invariants (non-negotiable)

- Lock ordering: PROCESS_TABLE → per-process locks → SOCKETS
- CURRENT_PROCESS never held across blocking operations
- PID 1 is immune to OOM killer and cannot be killed
- Orphans are reparented to init on parent exit
- FPU state saved/restored on every context switch (XSAVE/XRSTOR)
- SIGKILL cannot be caught, blocked, or ignored
- Clone flags: CLONE_VM shares address space, CLONE_FILES shares FD table

## Migration Guide

1. Extract `process/types.rs` → standalone (no deps)
2. Extract `oom.rs` → depends on `vahi_types::ProcessProvider`
3. Extract `scheduler/` → depends on `vahi_types` + `vahi-sync`
4. Extract `thread.rs` → depends on `vahi_types` + `x86_64`
5. Extract `process.rs` last (largest, most coupled)
