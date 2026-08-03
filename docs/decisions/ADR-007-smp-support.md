# ADR-007: SMP Support via SIPI + Per-CPU Schedulers

## Status
Accepted

## Date
2026-08-01

## Context
The kernel needed symmetric multiprocessing support for utilizing multiple CPU cores. Key constraints:
- Boot APs via SIPI (x86_64 mechanism)
- Each CPU needs its own scheduler instance and interrupt controller
- Threads must be migratable between CPUs for load balancing
- Work stealing for idle CPUs
- Minimal cache line bouncing on scheduler data

## Decision
Implement SMP with the following design:
1. **AP Boot**: BSP sends SIPI to APs with a trampoline at physical address 0x8000. APs enter protected mode, load their GDT/IDT, and enter the kernel's `ap_main`.
2. **Per-CPU Schedulers**: Each CPU has its own `Mutex<PerCpuScheduler>` with `MAX_CPUS = 8`. The global `GLOBAL` struct holds shared queues (pending, sleep, block, futex), each with its own Mutex to minimize contention.
3. **Work stealing**: When a CPU's local stride heap and global pending queue are both empty, it attempts to steal from up to 3 other CPUs' stride heaps.
4. **Reschedule IPI**: When threads are woken (futex, pipe), a broadcast IPI is sent so other CPUs can try_schedule().

## Alternatives Considered

### Single global scheduler with one lock
- Pros: Simple, no per-CPU data structures
- Cons: Cache line bouncing on every scheduling decision, contention under load
- Rejected: Does not scale to 2+ CPUs

### Lock-free per-CPU queues
- Pros: No spinlock contention
- Cons: Complex correctness proofs, difficult to implement correctly in no_std
- Rejected: Not worth the risk for initial SMP implementation

## Consequences
- Each CPU independently picks the next thread from its stride heap (O(log n))
- Work stealing balances load without central coordination
- Each queue has its own lock (pending, sleep, block, futex) — independent operations don't contend
- The scheduler is linear in CPU count, not thread count
- Preemption via LAPIC timer interrupt fires try_schedule() on each CPU