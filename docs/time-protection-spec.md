# Stride Scheduler: Time Protection Formal Specification

## 1. Abstract Model

We model the scheduler as a labeled transition system:

- **State**: (pass_i, stride_i, tickets_i) for each thread i, plus the set of ready threads R
- **Transition**: τ: pick thread with min pass, advance pass_i += stride_i
- **Initial state**: all pass_i = 0, tickets_i = DEFAULT_TICKETS = 20

## 2. Invariant: No Information Leakage via Timing

**Theorem (Time Protection)**: For any two threads A, B with tickets_A >> tickets_B,
the interleaving of A's execution (as observed by B through timing measurements)
is determined solely by the scheduler's deterministic stride ordering, not by
A's memory access patterns or cache state.

### Proof Obligations:

**PO1 (Deterministic Dispatch Order)**:
Given the same initial (pass, stride) state, the sequence of thread selections
is deterministic and independent of thread behavior during execution.

**PO2 (Quantized Time Slices)**:
Each thread runs for exactly one timer tick (10ms) before the scheduler
re-evaluates. No thread can extend its slice by any action.

**PO3 (Cache State Non-Propagation)**:
Between context switches, the L1/L2 caches are tagged with PCID (Process
Context Identifier — x86_64 PCID feature). A thread cannot read another
thread's cache lines.

### Formal Verification Approach:

The stride scheduler's properties can be proven using:

1. **Abstract interpretation** of the stride formula:
   - pick: argmin_i pass_i
   - advance: pass_i := pass_i + stride_i where stride_i = STRIDE_MAX / tickets_i

2. **Bounded model checking** (via CBMC or Kani) for N threads:
   - Prove that for any sequence of timer interrupts, the dispatch order
     matches the stride schedule specification
   - Prove that the maximum deviation from ideal proportional share is bounded

3. **Timing channel analysis**:
   - Measure time between context switches to B: this is exactly the stride
     of A + stride of B + ... for all threads that ran between
   - Since stride values are public (derived from tickets), B learns nothing
     about A's behavior from the timing

## 3. Security Model

### Adversarial Capabilities:
- Attacker controls thread B
- Attacker can measure wall-clock time between B's time slices
- Attacker knows the ticket values of all threads
- Attacker CANNOT: read kernel memory, observe A's cache state, observe A's I/O

### Guarantee:
The time between B's consecutive time slices is bounded by:
  max_between = (sum_of_all_strides) / min_stride × tick_length

Since this depends only on public information (ticket counts), B learns nothing
about A's internal state.

## 4. seL4 Comparison

seL4 proves:
1. **Integrity**: No untrusted code can affect the kernel's protected state
2. **Confidentiality**: No unauthorized information flow between partitions
3. **Availability**: Each partition receives its guaranteed CPU budget

Our stride scheduler provides:
- **Deterministic proportional share** (seL4's availability guarantee)
- **Quantized preemption** prevents covert timing channels (confidentiality)
- **PCID-based cache isolation** prevents cache side channels

### Remaining Work:
- [ ] Formal proof using Kani Rust Verifier for the dispatch loop
- [ ] Machine-checked proof that stride wrapping preserves ordering
- [ ] Measurement of actual timing jitter on real hardware
