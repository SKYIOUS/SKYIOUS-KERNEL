# Invariants: `memory` Module

**Last verified:** 2026-08-29
**Verified by:** Invariant-driven design analysis

---

## Critical Invariants

### INV-M1: Frame Ownership

**Statement:** Every physical frame has exactly one owner (the frame allocator or a single process) until explicitly shared via CoW or mapping.

**Boundary:** Frame allocation, mapping, CoW clone, deallocation

**Enforcement:**
- `alloc_frame()` gives ownership to caller
- `clone_cow()` increments refcount (shared ownership)
- `handle_cow()` decrements refcount on copy (restores single ownership)
- `destroy()` decrements refcount on free

**Verification:**
- Unit test: `memory:phys_alloc` verifies allocation
- Unit test: `memory:phys_free_reuse` verifies reuse
- `frame_info` module tracks refcounts

**Failure mode:** If refcount is wrong, a frame could be freed while still mapped (use-after-free) or never freed (memory leak). The `drain_deferred()` mechanism prevents TOCTOU races.

---

### INV-M2: Refcount Accuracy

**Statement:** The refcount for a frame matches the number of page table entries pointing to it plus 1 (for the allocator's reference).

**Boundary:** Mapping, unmapping, CoW, deallocation

**Enforcement:**
- `increment()` called when a frame is mapped
- `decrement()` called when a frame is unmapped or CoW-copied
- `drain_deferred()` frees frames with refcount 0

**Verification:**
- `frame_info::count()` returns current refcount
- `drain_deferred()` processes deferred-free queue

**Failure mode:** If increment/decrement are not balanced, frames leak or are freed too early. The deferred-free mechanism provides a safety net.

---

### INV-M3: CoW Consistency

**Statement:** After CoW clone, both parent and child have read-only mappings. First write triggers CoW fault. After fault, the writing process has a private copy.

**Boundary:** Fork, page fault

**Enforcement:**
- `clone_cow()` marks all user pages as read-only + CoW bit (bit 9)
- `handle_cow()` allocates new frame, copies data, marks writable

**Verification:**
- Unit test: `address_space:cow_clone` verifies clone creates separate address space
- Code inspection: handle_cow checks CoW bit before resolving

**Failure mode:** If CoW bit is not set, writes would modify the parent's page (data corruption). If handle_cow doesn't copy data, the child would see stale data.

---

### INV-M4: TLB Consistency

**Statement:** Every PTE modification is followed by a TLB flush on the current CPU. On SMP, a TLB shootdown is broadcast to all CPUs.

**Boundary:** PTE update, context switch

**Enforcement:**
- `handle_cow()` calls `tlb::flush()` after PTE update
- `broadcast_tlb_flush()` sends IPI to other CPUs (SMP)

**Verification:**
- Code inspection: flush in both CoW branches
- SMP feature gate: `#[cfg(feature = "smp")]`

**Failure mode:** If TLB is not flushed, the CPU would use the old (stale) mapping, causing data corruption or page faults.

---

### INV-M5: Deferred Free Safety

**Statement:** Frames with refcount 0 are queued for deferred free, not immediately deallocated. This prevents TOCTOU races with concurrent CoW handlers.

**Boundary:** CoW fault, frame deallocation

**Enforcement:**
- `decrement()` uses AtomicU16 (lock-free) and queues frames with refcount 0 to `deferred` Vec (behind its own Mutex)
- `drain_deferred()` processes the queue outside the refcount lock

**Verification:**
- Code inspection: decrement pushes to deferred, not to buddy
- Code inspection: drain_deferred acquires buddy lock separately

**Failure mode:** If a frame is deallocated immediately after refcount reaches 0, another CPU could still be using it (via a stale page table entry). The deferred-free mechanism prevents this.

---

### INV-M6: No Allocation in Fault Path

**Statement:** The page fault handler must not allocate heap memory. It runs with interrupts disabled (IF=0).

**Boundary:** Page fault handler

**Enforcement:**
- `page_fault_handler` uses only stack-allocated buffers
- `handle_cow` uses `BuddyFrameAllocator` (not heap allocation)
- `cow_stats_update` uses `try_lock()` (not `lock()`)

**Verification:**
- Code inspection: no `Vec::new()`, `String::new()`, or `format!()` in fault path
- `IrqFmtBuf` for debug output (stack-allocated)

**Failure mode:** If the fault handler allocates heap memory, it could trigger a recursive page fault (infinite loop) or deadlock (if the allocator needs a lock that's already held).

---

## Lock Ordering (Within Memory Module)

```
CURRENT_PROCESS → REFCOUNTS → BUDDY_ALLOCATOR
```

**This ordering is CRITICAL.** The page fault handler holds CURRENT_PROCESS and calls into REFCOUNTS (via handle_cow → frame_info::count). The deferred free path holds REFCOUNTS and calls into BUDDY_ALLOCATOR (via drain_deferred). These orderings must never be reversed.

**Verification:**
- Code inspection: all call sites follow this ordering
- Invariant analysis: no reverse dependencies found

**Failure mode:** Reversing the ordering causes ABBA deadlock. On a single CPU, this manifests as a hang with interrupts disabled. On SMP, it's a multi-CPU deadlock.
