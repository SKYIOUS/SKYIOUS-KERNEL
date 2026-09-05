# Module Interface: `memory`

**Path:** `kernel/src/memory/`
**Owner:** TBD
**Tier:** 1 (depends on sync, arch)

---

## Public API

### Physical Frame Allocation (`memory/phys.rs`)

```rust
/// Allocate a single physical frame. Returns None if no frames available.
pub fn alloc_frame() -> Option<PhysAddr>

/// Free a physical frame.
pub fn free_frame(addr: u64)

/// Check if a frame is free.
pub fn is_free(addr: u64) -> bool

/// Get total free frame count.
pub fn total_free_frames() -> u64
```

### Buddy Allocator (`memory/buddy.rs`)

```rust
/// Global buddy allocator.
pub static BUDDY_ALLOCATOR: IrqSafeMutex<BuddyFrameAllocator>

/// Allocate a single frame.
impl BuddyFrameAllocator {
    pub fn allocate_frame(&mut self) -> Option<PhysFrame>
    pub fn allocate_contiguous(&mut self, order: u8) -> Option<PhysAddr>
    pub fn deallocate_frame(&mut self, frame: PhysFrame)
    pub fn deallocate_contiguous(&mut self, addr: PhysAddr, order: u8)
    pub fn count_free_pages(&self) -> usize
}
```

### Address Space (`memory/paging.rs`)

```rust
/// Virtual address space (page tables).
pub struct AddressSpace {
    pml4_frame: PhysFrame,  // private
    pub cow_stats: CowStats,
}

impl AddressSpace {
    /// Create a new empty address space with kernel mappings.
    pub fn new(frame_allocator: &mut impl FrameAllocator) -> Option<Self>

    /// CoW-clone the address space (for fork).
    /// Returns a new AddressSpace with shared pages marked CoW.
    pub fn clone_cow(&self, frame_allocator: &mut BuddyFrameAllocator) -> Option<Self>

    /// Destroy the address space, freeing all user pages.
    pub unsafe fn destroy(&mut self)

    /// Activate this address space (write CR3).
    pub unsafe fn activate(&self)

    /// Get a mapper for this address space.
    pub unsafe fn mapper(&self) -> Option<OffsetPageTable>

    /// Handle a CoW page fault.
    /// Returns Some(true) if resolved, Some(false) if CoW failed, None if not CoW.
    pub unsafe fn handle_cow(&self, page: Page) -> Option<bool>
}

/// CoW statistics per address space.
pub struct CowStats {
    pub cow_faults: AtomicU64,
    pub pages_copied: AtomicU64,
    pub in_place_promotions: AtomicU64,
}
```

### Frame Info (`memory/frame_info.rs`)

```rust
/// Increment refcount for a physical frame.
pub fn increment(phys: PhysAddr)

/// Decrement refcount for a physical frame.
/// Returns remaining refcount. 0 = frame queued for deferred free.
pub fn decrement(phys: PhysAddr) -> u16

/// Get current refcount for a frame.
pub fn count(phys: PhysAddr) -> u16

/// Process deferred-free queue. Called from scheduler idle path.
pub fn drain_deferred()
```

---

## Invariants

1. **Frame ownership:** Every physical frame has exactly one owner (allocator or process) until explicitly shared via CoW.
2. **Refcount accuracy:** Refcount matches the number of page table entries pointing to the frame + 1 (for the allocator).
3. **CoW consistency:** After CoW clone, both parent and child have read-only mappings. First write triggers CoW fault.
4. **TLB consistency:** Every PTE modification is followed by a TLB flush.
5. **Deferred free safety:** Frames with refcount 0 are queued for deferred free, not immediately deallocated. This prevents TOCTOU races with concurrent CoW handlers.
6. **No allocation in fault path:** The page fault handler must not allocate heap memory (it runs with interrupts disabled).

---

## Lock Ordering (within memory module)

```
CURRENT_PROCESS → REFCOUNTS → BUDDY_ALLOCATOR
```

**This ordering is critical.** Violation causes deadlock. The page fault handler holds CURRENT_PROCESS and calls into REFCOUNTS (via handle_cow → frame_info::count). The deferred free path holds REFCOUNTS and calls into BUDDY_ALLOCATOR (via drain_deferred). These orderings must never be reversed.

---

## Testing Requirements

| Test | What It Validates | Priority |
|------|-------------------|----------|
| `memory:phys_alloc` | Physical frame allocation | ✅ Exists |
| `memory:phys_free_reuse` | Frame reuse after free | ✅ Exists |
| `memory:phys_double_free` | Double-free detection | ✅ Exists |
| `memory:buddy_alloc` | Buddy allocator | ✅ Exists |
| `memory:buddy_order` | Multi-page allocation | ✅ Exists |
| `address_space:create` | Address space creation | ✅ Exists |
| `address_space:cow_clone` | CoW clone | ✅ Exists |
| **NEW: memory:cow_fault** | CoW fault resolves correctly | ❌ Needed |
| **NEW: memory:cow_concurrent** | Concurrent CoW faults don't deadlock | ❌ Needed |
| **NEW: memory:exhaustion** | Allocation failure handled gracefully | ❌ Needed |

---

## Common Pitfalls

1. **Frame info lock in fault path:** `frame_info::count()` and `decrement()` acquire REFCOUNTS.lock() (disables interrupts). On SMP, contention could cause the fault handler to spin. Keep critical sections short.
2. **Deferred free:** Never call `drain_deferred()` from the page fault handler. It acquires BUDDY_ALLOCATOR which could deadlock with the fault path.
3. **Address space destroy:** Must be called with interrupts enabled (it acquires multiple locks). Never call from IRQ context.
4. **CoW stats:** Use `cow_stats_update()` which uses try_lock(). Never lock CURRENT_PROCESS directly in the fault path for stats.
