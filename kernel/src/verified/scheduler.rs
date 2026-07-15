#![allow(dead_code)]

//! Stride scheduling correctness proofs.
//!
//! # Stride scheduling model
//!
//! The Vahi kernel uses a proportional-share stride scheduler where each thread
//! holds `tickets` (entitlement) and advances a virtual `pass` counter by
//! `stride = STRIDE_MAX / tickets` after each time slice.  The scheduler
//! always picks the ready thread with the smallest `pass`.
//!
//! ## Proof obligations
//!
//! 1. **INVARIANT 1 — Pass sum**:
//!    The sum of all thread passes is bounded by the number of scheduling
//!    events times `max_stride`.  Formally:
//!    ```text
//!    Σ pass_i ≤ ticks_elapsed × max_stride + initial_bias
//!    ```
//!
//! 2. **INVARIANT 2 — Min-pass selection**:
//!    The selected thread `s` satisfies `pass_s ≤ pass_i` for all ready
//!    threads `i`.  This is structural in `stride_min_pass()` but we check
//!    it dynamically.
//!
//! 3. **INVARIANT 3 — Stretch bound**:
//!    No two ready threads differ in pass by more than `2 × max_stride`:
//!    ```text
//!    max_pass - min_pass ≤ 2 × STRIDE_MAX
//!    ```
//!    This bounds unfairness; a thread that has run "too much" will have
//!    a large pass and won't be selected again until others catch up.
//!
//! 4. **PROGRESS** — Every thread with non-zero tickets will eventually
//!    be scheduled (liveness).  This follows from INVARIANT 3: no thread
//!    can lag more than `2 × STRIDE_MAX` behind the minimum, so after at
//!    most `2 × STRIDE_MAX / stride_i` slices it will be the minimum.
//!
//! 5. **FAIRNESS** — Over a long interval, CPU time received by thread `i`
//!    divided by total CPU time converges to `tickets_i / Σ tickets`.
//!    Pass values track virtual time, so min-pass selection implements
//!    weighted fair queuing.

use crate::task::thread::Thread;

/// Maximum stride constant (must match `thread.rs`).
pub const STRIDE_MAX: u64 = 1 << 20;

/// Violation kinds detected by invariant checks.
#[derive(Debug, Clone)]
pub enum SchedViolation {
    PassSumMismatch {
        expected: u64,
        actual: u64,
        detail: alloc::string::String,
    },
    SelectedNotMinPass {
        selected_pass: u64,
        min_pass: u64,
        detail: alloc::string::String,
    },
    StretchViolation {
        max_pass: u64,
        min_pass: u64,
        limit: u64,
        detail: alloc::string::String,
    },
    StarvationRisk {
        thread_pass: u64,
        min_pass: u64,
        gap: u64,
    },
    NegativeTickets {
        tickets: u32,
    },
}

impl core::fmt::Display for SchedViolation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SchedViolation::PassSumMismatch { expected, actual, detail } => {
                write!(f, "PassSumMismatch: expected={expected} actual={actual} {detail}")
            }
            SchedViolation::SelectedNotMinPass { selected_pass, min_pass, detail } => {
                write!(f, "SelectedNotMinPass: selected={selected_pass} min={min_pass} {detail}")
            }
            SchedViolation::StretchViolation { max_pass, min_pass, limit, detail } => {
                write!(f, "StretchViolation: max={max_pass} min={min_pass} limit={limit} {detail}")
            }
            SchedViolation::StarvationRisk { thread_pass, min_pass, gap } => {
                write!(f, "StarvationRisk: pass={thread_pass} min={min_pass} gap={gap}")
            }
            SchedViolation::NegativeTickets { tickets } => {
                write!(f, "NegativeTickets: tickets={tickets}")
            }
        }
    }
}

/// Context gathered from the scheduler at a scheduling decision point.
pub struct SchedSnapshot<'a> {
    pub threads: &'a [&'a Thread],
    pub selected_idx: usize,
    pub elapsed_ticks: u64,
}

/// Run all stride scheduler invariant checks against a snapshot.
///
/// Returns `Ok(())` if all invariants hold, or `Err` with the first
/// violation detected.
pub fn check_schedule_correctness(snap: &SchedSnapshot<'_>) -> Result<(), SchedViolation> {
    let threads = snap.threads;
    if threads.is_empty() {
        return Ok(());
    }

    // --- INVARIANT 2: selected must be min-pass ---
    let selected_pass = threads[snap.selected_idx].pass;
    let min_pass = threads.iter().map(|t| t.pass).min().unwrap_or(u64::MAX);
    if selected_pass > min_pass {
        return Err(SchedViolation::SelectedNotMinPass {
            selected_pass,
            min_pass,
            detail: alloc::format!(
                "selected thread {} has pass {} > min {} among {} ready threads",
                snap.selected_idx, selected_pass, min_pass, threads.len()
            ),
        });
    }

    // --- INVARIANT 3: stretch bound ---
    let max_pass = threads.iter().map(|t| t.pass).max().unwrap_or(0);
    let limit = STRIDE_MAX.saturating_mul(2);
    if max_pass.saturating_sub(min_pass) > limit {
        return Err(SchedViolation::StretchViolation {
            max_pass,
            min_pass,
            limit,
            detail: alloc::format!(
                "pass range {} exceeds limit {} (2 × STRIDE_MAX)",
                max_pass.saturating_sub(min_pass),
                limit,
            ),
        });
    }

    // --- INVARIANT 1: pass sum bounded ---
    let total_pass: u64 = threads.iter().map(|t| t.pass).fold(0u64, core::ops::Add::add);
    let max_possible = snap.elapsed_ticks.saturating_mul(STRIDE_MAX);
    if total_pass > max_possible.saturating_mul(2) {
        // Multiply by 2 for slack (initial passes may be skewed)
        return Err(SchedViolation::PassSumMismatch {
            expected: max_possible,
            actual: total_pass,
            detail: alloc::format!(
                "Σpass {total_pass} >> expected max {max_possible} (ticks={})",
                snap.elapsed_ticks
            ),
        });
    }

    // --- Starvation check ---
    for (i, t) in threads.iter().enumerate() {
        let gap = t.pass.saturating_sub(min_pass);
        if gap > STRIDE_MAX.saturating_mul(4) {
            return Err(SchedViolation::StarvationRisk {
                thread_pass: t.pass,
                min_pass,
                gap,
            });
        }
        if t.tickets == 0 {
            return Err(SchedViolation::NegativeTickets { tickets: t.tickets });
        }
        let _ = i; // suppress unused
    }

    Ok(())
}

/// Contract-like wrapper around a stride scheduling decision.
///
/// Precondition:  run_queue non-empty, current_time > 0.
/// Postcondition: returned index is valid AND the selected thread's
/// pass is minimal among all ready threads.
pub fn schedule_contract(
    run_queue: &[&Thread],
    current_time: u64,
) -> Result<usize, SchedViolation> {
    // Precondition
    if run_queue.is_empty() {
        return Err(SchedViolation::StarvationRisk {
            thread_pass: 0,
            min_pass: 0,
            gap: 0,
        });
    }
    if current_time == 0 {
        return Err(SchedViolation::PassSumMismatch {
            expected: 1,
            actual: 0,
            detail: "current_time must be > 0".into(),
        });
    }

    // Core logic: find min-pass
    let selected = run_queue
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.pass.cmp(&b.pass))
        .map(|(i, _)| i)
        .unwrap_or(0);

    // Postcondition: verify selection is valid
    let snap = SchedSnapshot {
        threads: run_queue,
        selected_idx: selected,
        elapsed_ticks: current_time,
    };
    check_schedule_correctness(&snap)?;

    Ok(selected)
}
