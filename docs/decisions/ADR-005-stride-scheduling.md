# ADR-005: Stride Scheduling with Proportional Fairness

## Status
Accepted

## Date
2026-08-01

## Context
The kernel needed a CPU scheduling algorithm that provides:
- Strong proportional fairness (CPU time proportional to thread tickets/priority)
- No starvation (every thread with tickets > 0 runs infinitely often)
- O(log n) scheduling decisions
- Deterministic dispatch order for timing-channel resistance
- Priority support (8 levels) for real-time-like behavior

## Decision
Use Stride scheduling as the primary algorithm, implemented as a BinaryHeap keyed by `pass` value.

Each thread has:
- `tickets` (default 20) — proportional share weight
- `stride = STRIDE_MAX / tickets` — virtual time advancement per tick (STRIDE_MAX = 2^20)
- `pass` — accumulated virtual time, starts at 0

At each scheduling decision, the thread with the smallest pass runs, then advances `pass += stride`. The scheduler uses 8 legacy priority queues for compatibility (`wake_*`, `tick()` APIs), which are drained into the stride heap before selection.

A formal proof of fairness is documented in `docs/stride-formal-proof.md`.

## Alternatives Considered

### CFS (Completely Fair Scheduler, Linux-style)
- Pros: Well-studied, handles interactive workloads well
- Cons: More complex (red-black tree), requires virtual runtime tracking with nice values
- Rejected: Stride is simpler and sufficient for our workload model

### Fixed-priority preemptive (strict priority only)
- Pros: Simple O(1) scheduling
- Cons: Starvation of low-priority threads, no proportional fairness
- Rejected: Starvation is unacceptable for a general-purpose kernel

### Round-robin with time slices
- Pros: Simplest possible, fair at coarse granularity
- Cons: No proportional allocation, no priority support by default
- Rejected: Need proportional fairness

## Consequences
- O(log n) scheduling decision (BinaryHeap push/pop)
- Deterministic ordering prevents timing side-channels (see `docs/time-protection-spec.md`)
- Work stealing between CPUs maintains fairness on SMP
- Legacy priority queues are drained into stride heap at pick_next time
- Blocked threads in sleep/futex/pipe queues don't accumulate pass during idle