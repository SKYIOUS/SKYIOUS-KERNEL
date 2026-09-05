# vahi-memory

Memory management for the Vahi kernel.

## Extracted Code

| Module | Lines | Contents |
|--------|-------|----------|
| `frame_info` | 122 | Atomic refcounts (AtomicU16), allocation tracking |

## Still in Kernel

| Module | Lines | Why |
|--------|-------|-----|
| `buddy.rs` | 225 | Depends on swap + physical_memory_offset |
| `phys.rs` | 48 | Thin wrapper around buddy |
| `virt.rs` | 74 | x86_64 types only |
| `slab.rs` | 125 | Depends on buddy |
| `paging.rs` | 397 | Depends on task (CURRENT_PROCESS) |
| `swap.rs` | 85 | Depends on frame_info + buddy |
| `stack.rs` | 75 | Depends on vahi-sync |
| `isolate.rs` | 230 | x86_64 types |
| `aarch64.rs` | 375 | cfg-gated |

## Migration Order

Extract as a group (tightly coupled):
1. `buddy.rs` → depends on frame_info
2. `phys.rs` → thin wrapper
3. `virt.rs` → standalone
4. `slab.rs` → depends on buddy
5. `stack.rs` → depends on vahi-sync
6. `swap.rs` → depends on frame_info + buddy
7. `paging.rs` → depends on everything + task
8. `isolate.rs` → x86_64 types
9. `aarch64.rs` → cfg-gated

## Invariants

- Refcounts are `AtomicU16` (lock-free hot path)
- Lock ordering: `CURRENT_PROCESS` → `REFCOUNTS` → `BUDDY_ALLOCATOR`
- `drain_deferred()` must NOT be called while holding REFCOUNTS or BUDDY
- TLB must be flushed after every CoW PTE update
