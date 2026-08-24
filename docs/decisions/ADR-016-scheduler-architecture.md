# ADR-016: Scheduler Architecture

## Status

**DECISION REQUIRED** — This ADR proposes a scheduler architecture that requires team consensus.

## Context

The current scheduler is a **stride (proportional-share) scheduler** with:
- Per-CPU BinaryHeap keyed by `pass` (virtual time)
- 8 priority levels (legacy `ready_queues`)
- Work stealing when own heap is empty
- Global queues for pending/sleep/block/futex threads

The scheduler has 4 correctness defects:
1. No pass normalization (unbounded growth)
2. Work stealing only when own heap empty (poor SMP)
3. Priority inversion (stub implementation)
4. No close-on-exec filtering

Linux moved from CFS to EEVDF in 6.6. EEVDF provides:
- Better latency for interactive tasks
- Support for deadlines (not just fairness)
- Eligibility mechanism to prevent gaming

## Decision

**Keep stride scheduling for now. Migrate to EEVDF when Linux compatibility is needed.**

### Rationale

1. **Stride is functional** — It works for single-CPU and basic SMP workloads
2. **Stride is simple** — Easy to understand, debug, and verify
3. **EEVDF is complex** — More moving parts, harder to verify correctness
4. **Linux compatibility is not yet required** — Focus on foundation first

### Migration Path

**Phase 1: Fix Stride Defects (Immediate)**
- Add pass normalization
- Add periodic load balancing
- Add priority inheritance for futex

**Phase 2: Add EEVDF Eligibility (When needed)**
- Add eligibility check to stride scheduler
- Add virtual deadline tracking
- Test with real workloads

**Phase 3: Replace Stride with EEVDF (When Linux compat required)**
- Replace BinaryHeap with BTreeMap sorted by deadline
- Implement full EEVDF algorithm
- Benchmark against stride

### New Types (Phase 2)

```rust
/// EEVDF eligibility and deadline tracking.
pub struct EevdfState {
    /// Virtual deadline for scheduling.
    pub virtual_deadline: u64,
    /// Whether thread is eligible to run.
    pub eligible: bool,
    /// Lag (how much the thread has been under-served).
    pub lag: i64,
}

/// Enhanced thread with EEVDF support.
pub struct Thread {
    // ... existing fields ...
    pub eevdf: EevdfState,
}
```

### New Scheduler API (Phase 2)

```rust
impl PerCpuScheduler {
    /// Pick next thread using EEVDF eligibility + stride ordering.
    pub fn pick_next_eevdf(&mut self) -> Option<Box<Thread>> {
        self.flush_ready_queues();
        
        // Find eligible threads (pass <= virtual_time)
        let virtual_time = self.get_virtual_time();
        
        // Among eligible threads, pick min-deadline
        // If no eligible threads, pick min-pass (will become eligible soon)
        self.stride_heap.pop().map(|p| p.0)
    }
    
    /// Update eligibility after each time slice.
    pub fn update_eligibility(&mut self) {
        let virtual_time = self.get_virtual_time();
        for t in self.stride_heap.iter_mut() {
            t.0.eevdf.eligible = t.0.pass <= virtual_time;
        }
    }
}
```

## Consequences

### Positive

1. **No behavioral change** — Stride continues to work as before
2. **Incremental migration** — Can add EEVDF features one at a time
3. **Easy rollback** — If EEVDF causes issues, fall back to stride
4. **Linux compatibility path** — EEVDF enables future Linux ABI compatibility

### Negative

1. **Two code paths** — Stride and EEVDF coexist during migration
2. **Complexity** — More code to maintain and test
3. **Performance overhead** — Eligibility checks add overhead

### Risks

1. **EEVDF may not improve Vahi workloads** — Linux workloads are different
2. **Migration may break existing behavior** — Need comprehensive tests
3. **Complexity may introduce bugs** — EEVDF is harder to verify

## Alternatives Considered

### Alternative 1: Keep Stride Forever

**Rejected.** Stride cannot support real-time guarantees or Linux compatibility. When those become requirements, stride will need to be replaced.

### Alternative 2: Migrate to EEVDF Immediately

**Rejected.** EEVDF is complex and untested in Vahi. Migrate only when needed.

### Alternative 3: Use Linux's CFS

**Rejected.** CFS is deprecated in Linux (replaced by EEVDF). Don't migrate to deprecated code.

## Implementation Plan

1. **Phase 1 (Immediate):** Fix stride defects (pass normalization, load balancing, priority inheritance)
2. **Phase 2 (When needed):** Add EEVDF eligibility to stride scheduler
3. **Phase 3 (When Linux compat required):** Replace stride with full EEVDF

## References

- Linux EEVDF: `kernel/sched/fair.c` (Linux 6.6+)
- Stride scheduling: Waldspurger & Weihl (1994)
- `docs/roadmap-revised/architecture-review/scheduler.md` — Full scheduler audit
