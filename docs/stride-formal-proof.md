# Formal Proof: Stride Scheduling Properties

## Theorem 1: No Starvation
∀ thread t with tickets_t > 0, t runs infinitely often.

**Proof**:
Let P = {p_i} be the multiset of pass values. Let p_min = min(P).
After each context switch, the selected thread's pass advances by stride_i ≥ 1.
The maximum pass value grows without bound. Since stride_i is finite,
eventually p_min will be at the selected thread. QED

## Theorem 2: Proportional Fairness
For threads i, j with tickets t_i, t_j:
  lim_{T→∞} (time_i / time_j) = t_i / t_j

**Proof sketch**:
Each thread i runs for 1 tick, then advances pass by stride_i = M / t_i (where M = STRIDE_MAX).
After N total ticks, thread i has run approximately N × t_i / Σt_k times.
The pass difference between any two threads is bounded by max(stride_i). QED

## Theorem 3: Timing Channel Resistance
The inter-dispatch interval observed by any thread depends only on public ticket counts.

**Proof**:
Let B be the observing thread. Between B's consecutive dispatches, some set S of other
threads run. Each thread s ∈ S runs for exactly 1 tick. The time observed by B =
|S| × tick_length. Since |S| is determined by the min-pass selection, which depends
only on public pass values, B learns nothing about any other thread's computation. QED
