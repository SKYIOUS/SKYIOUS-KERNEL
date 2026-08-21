# ASH — Application-Specific Safe Handlers

## Overview

ASH (Application-Specific Safe Handlers) lets privileged userspace attach
small eBPF programs to kernel hook points (network receive, syscall entry).
Handlers are verified before installation and executed by the kernel's eBPF
interpreter; they never run native code (no JIT — see below).

Implementation lives in `kernel/src/ash/` and is compiled with the `ash`
feature flag.

## Lifecycle

```
Register → Verify → Install → Execute (hook point) → Unregister
```

1. **Register**: userspace calls `SYS_ASH_REGISTER` (310) with a raw eBPF
   bytecode buffer (an array of `EbpfInsn`, 8 bytes each) and a packed
   `hook_info` value describing the hook point. Requires euid 0 or
   CAP_SYS_ADMIN (bit 13) / CAP_SYS_PTRACE (bit 21).

2. **Verify**: `ash::verifier::verify_handler` runs the structural eBPF
   verifier plus tnum abstract interpretation, then checks hook
   compatibility (memory accesses stay within the hook's context size).
   Limits: ≤ 512 instructions, memory budget 512 bytes. Rejected handlers
   return `EINVAL`.

3. **Install**: the handler is stored in the manager's tables
   (`ASH_MANAGER`, a BTreeMap keyed by handler id) together with its
   verified form. Duplicate registrations (same pid + bytecode + hook) are
   rejected.

4. **Execute**: when the corresponding kernel event fires, every matching
   handler runs via the eBPF interpreter (`ash::runtime::execute_handler`)
   with R1 = context struct pointer, R2 = payload pointer, R3 = payload
   length. Return value R0 maps to an `AshResult`.

5. **Unregister**: `SYS_ASH_UNREGISTER` (311) removes a handler by id
   (owner-only). `SYS_ASH_STATS` (312) exposes counters, `SYS_ASH_CONTROL`
   (313) is reserved for future control operations.

## Hook points

`ash::HookPoint` enumerates the attach points:

| HookPoint | Context size | Where it fires |
|-----------|-------------|----------------|
| `NetReceive { interface, port, protocol }` | 32 B | `recvfrom_internal` in syscalls (TCP arm protocol 6, UDP arm protocol 17) after `recv_slice` |
| `NetTransmit { interface, port, protocol }` | 32 B | reserved (not yet wired) |
| `SyscallEntry { syscall_num }` | 64 B | `do_syscall` in `syscalls/mod.rs`, before the dispatch match |
| `SyscallExit { syscall_num }` | 64 B | reserved (not yet wired) |
| `TimerFired { timer_id }` | 16 B | reserved |
| `SignalDelivery { signal }` | 16 B | reserved |
| `MessageReceive { channel }` | 32 B | reserved |

`hook_info` packing (rdx of `SYS_ASH_REGISTER`):
`[u8 hook_type, u8 protocol, u16 port, u32 arg]` — protocol/port apply to
net hooks, the u32 arg is the syscall number (SyscallEntry) or other hook
identifier.

## Result semantics

Handlers return one of:

| R0 | AshResult | Net receive | Syscall entry |
|----|-----------|-------------|---------------|
| 0  | Continue  | data delivered | syscall proceeds |
| 1  | Handled   | stops later handlers, data delivered | syscall denied (`EPERM`) |
| 2  | Drop      | data discarded (`EAGAIN` to caller) | syscall denied (`EPERM`) |
| 3  | Modified  | data may have been edited in place | syscall proceeds |
| ≥4 | Error     | data delivered | syscall proceeds |

## Context ABI

Network context (`NetContext`, `#[repr(C)]`, 32 bytes):

```
offset 0  u8  interface
offset 1  u8  protocol
offset 2  u16 src_port
offset 4  u16 dst_port
offset 6  u8  _pad[26]
```

Syscall context is 64 bytes (reserved layout, populated in a later step).

## Interpreter-only execution (no JIT)

A previous JIT path transmuted a heap-allocated bytecode buffer into a
function pointer and called it — a non-executable (NX) memory violation on
real hardware. The JIT path (`ash::jit.rs`, `runtime::execute_handler_jit`)
has been removed; all handlers run through the interpreter until a real
executable-memory allocator exists (see implementation plan A5).

## Kernel call sites

- `syscalls/mod.rs::do_syscall` — `hook_syscall_entry` (feature `ash`).
- `syscalls/mod.rs::recvfrom_internal` — `hook_net_receive` (feature `ash`),
  both TCP and UDP arms.

## Selftest

`ash::net_hook_fires` (tests/new_features.rs): registers a two-instruction
handler (`r0 += 2; exit` → Drop) on UDP port 9999, fires a synthetic packet
through `hook_net_receive`, asserts the result is `Drop`, then unregisters.