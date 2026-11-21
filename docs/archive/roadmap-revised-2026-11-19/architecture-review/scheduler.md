# Scheduler — Deep Correctness Audit

## Executive Summary

The Vahi scheduler is a **stride (proportional-share) scheduler** with per-CPU heaps and work stealing. It has **4 correctness defects**, **3 performance issues**, and **2 architectural limitations** that must be addressed before it can support production workloads.

**Overall Grade: B-** — Functional for single-CPU, broken for SMP.

---

## Architecture Overview

```text
                    ┌─────────────────────┐
                    │   Global Queues     │
                    │  ┌───────────────┐  │
                    │  │ pending_queue │  │  ← new threads (drained first)
                    │  ├───────────────┤  │
                    │  │ sleep_queue   │  │  ← timer-based wakeup
                    │  ├───────────────┤  │
                    │  │ block_queue   │  │  ← pipe/pipe-blocked
                    │  ├───────────────┤  │
                    │  │ futex_queue   │  │  ← futex-waiting
                    │  └───────────────┘  │
                    └─────────┬───────────┘
                              │
                    ┌─────────▼───────────┐
                    │   Per-CPU Schedulers │
                    │  ┌───────────────┐  │
                    │  │ stride_heap   │  │  ← BinaryHeap<PassOrd> (min-pass)
                    │  ├───────────────┤  │
                    │  │ ready_queues  │  │  ← [VecDeque; 8] (priority levels)
                    │  ├───────────────┤  │
                    │  │ switching_old │  │  ← thread being switched away
                    │  ├───────────────┤  │
                    │  │ idle          │  │  ← permanent idle thread
                    │  └───────────────┘  │
                    └─────────────────────┘
```

**Pick order:**
1. Global pending queue (new threads — drained first)
2. Stride heap (min-pass = highest priority)
3. Work stealing (other CPUs' stride heaps)

**Stride mechanics:**
- `pass` = accumulated virtual time (starts at 0)
- `stride` = `STRIDE_MAX / tickets` (tickets = proportional-share weight)
- `STRIDE_MAX` = 1 << 20 (1,048,576)
- Thread with lowest `pass` runs next
- After each time slice: `pass += stride`
- Lower tickets → higher stride → runs less often

---

## Correctness Findings

### Finding 1: No Pass Normalization (CRITICAL)

**Severity:** P0 — Unbounded memory growth, fairness degradation.

Pass values grow without bound via `wrapping_add`:
```rust
old.pass = old.pass.wrapping_add(old.stride);
```

After 2^64 time slices, pass values wrap to 0. But before that:
- Threads that ran recently have high pass values
- Threads that were blocked have low pass values
- When a blocked thread wakes up, it has a very low pass and starves others

**Attack scenario:**
1. Thread A runs for 1,000,000 time slices → pass = 1,000,000 × stride
2. Thread B is blocked on futex for a long time → pass = 0
3. Thread B wakes up → pass = 0, which is much lower than A's pass
4. Thread B runs for a very long time before A gets another turn

**Impact:** Fairness degrades over time. Long-running threads can be starved by recently-woken threads.

**Recommendation:** Normalize pass values periodically:
```rust
fn normalize_passes(heap: &mut BinaryHeap<PassOrd>) {
    let min_pass = heap.iter().map(|p| p.0.pass).min().unwrap_or(0);
    for t in heap.iter_mut() {
        t.0.pass = t.0.pass.wrapping_sub(min_pass);
    }
}
```

### Finding 2: Work Stealing Only When Own Heap Empty (HIGH)

**Severity:** P1 — Poor SMP load balancing.

Work stealing only triggers when the current CPU's stride heap is empty:
```rust
// 2. Stride heap
if let Some(PassOrd(t)) = self.stride_heap.pop() {
    return Some(t);  // ← takes from own heap first
}

// 3. Work stealing
for i in 0..MAX_CPUS {
    if let Some(mut other) = PER_CPU[i].try_lock() {
        if let Some(PassOrd(t)) = other.stride_heap.pop() {
            return Some(t);  // ← only steals when own is empty
        }
    }
}
```

This means:
- CPU 0 with 10 threads never shares with idle CPU 1
- CPU 1 sits idle while CPU 0 is overloaded
- No proactive rebalancing

**Impact:** Poor SMP utilization. Single-threaded workloads are fine, but mixed workloads suffer.

**Recommendation:** Add periodic load balancing (every N ticks):
```rust
fn periodic_load_balance() {
    let local_count = sched.stride_heap.len();
    for other_cpu in 0..MAX_CPUS {
        let other_count = PER_CPU[other_cpu].lock().stride_heap.len();
        if other_count > local_count + 2 {
            // Steal half the difference
            let to_steal = (other_count - local_count) / 2;
            for _ in 0..to_steal {
                if let Some(t) = PER_CPU[other_cpu].lock().stride_heap.pop() {
                    sched.stride_heap.push(t);
                }
            }
        }
    }
}
```

### Finding 3: Priority Inversion (HIGH)

**Severity:** P1 — Real-time guarantee violation.

`boost_thread_priority()` is a stub:
```rust
pub fn boost_thread_priority(_pid: u64, _target_priority: u8) -> bool {
    // ponytail: single-thread-per-process model; full priority inheritance
    // requires per-thread priority tracking across process boundaries.
    false
}
```

If a high-priority thread is blocked on a futex held by a low-priority thread:
- High-priority thread waits in `futex_queue`
- Low-priority thread runs at its own priority
- High-priority thread is effectively deprioritized

**Impact:** Real-time applications (audio, video) can miss deadlines.

**Recommendation:** Implement priority inheritance for futex:
```rust
fn futex_wait(uaddr: u64) {
    // Before blocking, boost the holder's priority
    if let Some(holder_pid) = find_futex_holder(uaddr) {
        boost_thread_priority(holder_pid, current_thread.priority);
    }
    // ... block ...
}
```

### Finding 4: No Close-on-Exec Filtering (MEDIUM)

**Severity:** P2 — POSIX compliance gap.

`clone_table()` for fork doesn't filter by `O_CLOEXEC`:
```rust
pub fn clone_table(&self) -> Vec<Option<HandleEntry>> {
    // ponytail: simple clone, no close-on-exec filtering needed yet
    self.table.clone()
}
```

This means all handles are inherited across fork+exec, violating POSIX semantics.

**Impact:** File descriptor leaks on exec. Security vulnerability (leaked sensitive FDs).

**Recommendation:** Filter during fork:
```rust
pub fn clone_for_fork(&self, close_on_exec: bool) -> Vec<Option<HandleEntry>> {
    self.table.iter().map(|slot| {
        if close_on_exec && slot.as_ref().map_or(false, |e| e.flags & O_CLOEXEC != 0) {
            None
        } else {
            slot.clone()
        }
    }).collect()
}
```

### Finding 5: Work Stealing Race Window (MEDIUM)

**Severity:** P2 — Theoretical race condition.

When work stealing, the current CPU locks the other CPU's scheduler:
```rust
if let Some(mut other) = PER_CPU[i].try_lock() {
    other.flush_ready_queues();
    if let Some(PassOrd(t)) = other.stride_heap.pop() {
        return Some(t);
    }
}
```

But `try_lock()` can fail if the other CPU is in `prepare_switch()` (holding the lock). In that case, the thread is skipped. This is correct behavior (not a race), but it means:
- High-priority threads can be temporarily invisible to work stealing
- This is a liveness issue, not a safety issue

**Impact:** Minor scheduling delay for high-priority threads on other CPUs.

### Finding 6: `switching_old` Not Visible to Other CPUs (LOW)

**Severity:** P3 — Design consideration.

`switching_old` holds the thread being switched away from. It's not in any queue, so other CPUs can't see it. This is correct (the thread is still executing), but it means:
- A thread in `switching_old` is invisible to work stealing
- If the current CPU crashes, the thread is lost

**Impact:** Theoretical data loss on CPU crash. Acceptable for now.

---

## Performance Findings

### Finding 7: No Periodic Load Balancing (HIGH)

**Severity:** P1 — SMP performance degradation.

Load balancing only happens when:
1. A thread is woken (`broadcast_reschedule_ipi`)
2. A CPU's own heap is empty (work stealing)

No periodic rebalancing means:
- Uneven CPU utilization
- Long-running threads stay on one CPU
- No migration of threads between CPUs

**Recommendation:** Add periodic load balancing every 10 ticks:
```rust
if tick_count % 10 == 0 {
    periodic_load_balance();
}
```

### Finding 8: IPI Broadcast on Every Wake (MEDIUM)

**Severity:** P2 — Interrupt overhead.

`broadcast_reschedule_ipi()` is called on every wake:
```rust
if woken > 0 { broadcast_reschedule_ipi(); }
```

This sends an IPI to all CPUs, even if they have no threads to run. With 8 CPUs and 100 wakes/sec, that's 800 IPIs/sec.

**Recommendation:** Batch IPIs or only send to CPUs with runnable threads.

### Finding 9: Linear Scan in `find_by_type` (LOW)

**Severity:** P3 — O(n) per handle lookup.

`HandleTable::find_by_type()` does a linear scan:
```rust
pub fn find_by_type(&self, type_id: ObjectTypeId) -> Vec<HandleValue> {
    self.table.iter().enumerate().filter_map(|(i, slot)| {
        slot.as_ref().and_then(|e| {
            if e.object.header().object_type == type_id { Some(i as HandleValue) } else { None }
        })
    }).collect()
}
```

With 1000+ handles, this is slow. Consider a type-indexed lookup table.

---

## Race Condition Analysis

### Safe Paths

1. **tick() → sleep_queue** — Uses `try_lock()`, defers on contention. ✅
2. **wake_*() → ready_queues** — Holds per-CPU mutex. ✅
3. **prepare_switch() → switching_old** — Only accessed by current CPU. ✅
4. **Work stealing → other CPU's heap** — Uses `try_lock()`. ✅

### Potentially Unsafe Paths

1. **route_outgoing() → AddressSpace::destroy()** — Called from idle stack. If the idle stack is too small, recursive page table destruction could overflow. **Recommendation:** Ensure idle stack ≥ 64KB.

2. **tick() → wake_process_futex_threads()** — Called from IRQ context with IF=0. The helper calls `broadcast_reschedule_ipi()` which sends IPIs. The IPI handler needs to run with IF=1, but the current CPU has IF=0. This is fine (other CPUs handle their own IPIs), but it means the current CPU won't process its own IPI until `sti`.

3. **fork() → clone_table() → handle_table** — Fork holds both parent and child handle table locks. If another thread on the same CPU tries to access the parent's handle table, it could deadlock. **Recommendation:** Use `try_lock()` for handle table access during fork.

---

## EEVDF Migration Path

### Why EEVDF?

Linux moved from CFS to EEVDF (Earliest Eligible Virtual Deadline First) in 6.6. EEVDF provides:
- Better latency for interactive tasks
- Support for deadlines (not just fairness)
- Eligibility mechanism to prevent gaming
- Compatible with Vahi's stride scheduling concept

### Migration Strategy

**Phase 1: Add Eligibility (Low Risk)**

Add an eligibility check to the stride scheduler:
```rust
impl PerCpuScheduler {
    fn pick_next_eligible(&mut self) -> Option<Box<Thread>> {
        self.flush_ready_queues();
        
        // Find eligible threads (pass <= current_time)
        let current_time = self.get_virtual_time();
        let eligible: Vec<_> = self.stride_heap.iter()
            .filter(|t| t.0.pass <= current_time)
            .collect();
        
        if eligible.is_empty() {
            // No eligible threads — pick min-pass (will become eligible soon)
            self.stride_heap.pop().map(|p| p.0)
        } else {
            // Pick min-pass among eligible
            self.stride_heap.pop().map(|p| p.0)
        }
    }
}
```

**Phase 2: Add Virtual Deadlines (Medium Risk)**

Extend `Thread` with deadline fields:
```rust
pub struct Thread {
    // ... existing fields ...
    pub virtual_deadline: u64,  // deadline for EEVDF scheduling
    pub eligible: bool,         // eligibility flag
}
```

**Phase 3: Replace Stride with EEVDF (High Risk)**

Replace the stride heap with an EEVDF queue:
```rust
pub struct EevdfScheduler {
    /// Ready threads sorted by virtual deadline
    ready_queue: BTreeMap<u64, VecDeque<Box<Thread>>>,
    /// Current virtual time
    virtual_time: u64,
}
```

### Migration Risks

1. **Behavioral change** — Existing workloads may see different scheduling behavior
2. **Complexity** — EEVDF is more complex than stride
3. **Testing** — Need comprehensive scheduler tests before migration

### Recommendation

**Keep stride for now.** It's functional and well-understood. Migrate to EEVDF only when:
- SMP load balancing is needed
- Real-time guarantees are required
- Linux compatibility is a priority

---

## Recommendations

### Immediate (P0)

1. **Add pass normalization** — Prevent unbounded pass growth
2. **Add close-on-exec filtering** — Fix POSIX compliance
3. **Verify idle stack size** — Ensure ≥ 64KB for destroy path

### Short-term (P1)

4. **Add periodic load balancing** — Every 10 ticks
5. **Add priority inheritance** — For futex operations
6. **Add work stealing threshold** — Steal when own heap < 50% of average

### Medium-term (P2)

7. **Add IPI batching** — Reduce interrupt overhead
8. **Add type-indexed handle lookup** — O(1) by type
9. **Add scheduler statistics** — Track latency, throughput, fairness

### Long-term (P3)

10. **EEVDF migration** — When Linux compatibility is needed
11. **CPU affinity** — Pin threads to specific CPUs
12. **NUMA awareness** — Prefer local memory allocations
