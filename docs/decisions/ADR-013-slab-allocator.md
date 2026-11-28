# ADR-013: Slab Allocator

## Status
Accepted

## Date
2026-08-15

## Context
The kernel's buddy allocator manages physical frames at page granularity (4 KiB). Kernel objects (process structures, file descriptors, VMA descriptors, etc.) are much smaller and allocating a full page for each causes severe internal fragmentation.

## Decision
Implement a slab allocator layered on top of the buddy allocator.

### Design
- Slab caches for common kernel object sizes (64, 128, 256, 512, 1024, 2048 bytes)
- Each cache maintains free lists of objects
- Buddy allocator provides backing pages (order 0 = 4 KiB)
- Per-CPU caches to reduce contention
- Full/partial/empty slab tracking

### Acceptance Criteria
- [ ] Reduce internal fragmentation for small allocations
- [ ] O(1) allocation/deallocation in hot paths
- [ ] Per-CPU caches reduce lock contention
- [ ] Integration with existing buddy allocator

## Consequences
- Adds ~800 lines to memory subsystem
- Requires careful handling of slab metadata overhead
- Must handle fragmentation within slab pages

## Alternatives Considered
### Keep buddy-only
- Pros: Simpler, no extra metadata
- Cons: Severe fragmentation for small objects, wasteful
- Rejected: Measured overhead was too high

### Use external allocator crate
- Pros: Less code to maintain
- Cons: Not no_std compatible, limited control
- Rejected: Kernel needs custom per-CPU caches