#![allow(dead_code)]

//! Lock correctness proofs.
//!
//! # Locking model
//!
//! The Vahi kernel uses two kinds of locks:
//!
//! - **`crate::sync::IrqSafeMutex`** — spinlock, used in interrupt handlers and hot
//!   paths where blocking is unsafe (e.g. `PerCpuScheduler`).
//! - **`SchedLock`** (`task::lock`) — sleep-wake lock that yields the
//!   scheduler on contention, used in VFS, compositor, and long-lived
//!   data-structure access.
//!
//! ## Proof obligations
//!
//! 1. **Mutual exclusion** — At most one thread holds a lock at any time.
//!    Enforced by hardware (atomic swap) for `crate::sync::IrqSafeMutex` and by the
//!    scheduler's pipe-block mechanism for `SchedLock`.
//!
//! 2. **No deadlock** — Lock acquisitions follow a global partial order.
//!    Violation: circular wait (thread A holds L1, waits L2; thread B
//!    holds L2, waits L1).
//!
//! 3. **Progress** — Every thread waiting for a lock eventually acquires
//!    it (no livelock, no unbounded priority inversion).
//!
//! 4. **Interrupt safety** — `crate::sync::IrqSafeMutex` is never held across a
//!    scheduling point in interrupt context (checked by the caller
//!    discipline: `try_lock` in timer/IRQ handlers).

use alloc::vec::Vec;
use alloc::string::String;

/// Opaque lock identifier for ordering analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockId(pub u64);

/// Thread identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThreadId(pub u64);

/// Reason a thread is blocked on a lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    HeldByOther(ThreadId),
    Contended,
    OrderViolation,
}

/// A recorded lock acquisition edge for cycle detection.
#[derive(Debug, Clone)]
pub struct LockEdge {
    pub from: LockId,
    pub to: LockId,
    pub thread: ThreadId,
}

/// Lock ordering graph node.
#[derive(Debug, Clone)]
pub struct LockNode {
    pub id: LockId,
    pub name: String,
    pub held_by: Vec<ThreadId>,
}

/// Violation detected by lock correctness checks.
#[derive(Debug, Clone)]
pub enum LockViolation {
    DeadlockDetected {
        cycle: Vec<LockId>,
        detail: String,
    },
    NoMutualExclusion {
        lock: LockId,
        holders: Vec<ThreadId>,
    },
    InterruptContextSpin {
        lock: LockId,
    },
    DoubleLock {
        lock: LockId,
        thread: ThreadId,
    },
    UnlockNotHeld {
        lock: LockId,
        thread: ThreadId,
    },
}

impl core::fmt::Display for LockViolation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LockViolation::DeadlockDetected { cycle, detail } => {
                write!(f, "DeadlockDetected: cycle of {} locks: {detail}", cycle.len())
            }
            LockViolation::NoMutualExclusion { lock, holders } => {
                write!(f, "NoMutualExclusion: lock {lock:?} held by {holders:?}")
            }
            LockViolation::InterruptContextSpin { lock } => {
                write!(f, "InterruptContextSpin: {lock:?} acquired in IRQ")
            }
            LockViolation::DoubleLock { lock, thread } => {
                write!(f, "DoubleLock: {lock:?} by thread {thread:?}")
            }
            LockViolation::UnlockNotHeld { lock, thread } => {
                write!(f, "UnlockNotHeld: {lock:?} by thread {thread:?}")
            }
        }
    }
}

/// Lock order verifier.
///
/// Uses a global lock-order graph to detect cycles (potential deadlocks)
/// via a simple DFS.  The graph is built at runtime from observed lock
/// acquisitions; in a full proof it would be derived from the static
/// lock hierarchy documentation.
pub struct LockOrderVerifier {
    /// Directed edges: `from ──► to` means "from must be acquired before to".
    edges: Vec<LockEdge>,
    /// Registered locks.
    locks: Vec<LockNode>,
}

impl LockOrderVerifier {
    pub const fn new() -> Self {
        LockOrderVerifier {
            edges: Vec::new(),
            locks: Vec::new(),
        }
    }

    /// Register a lock in the ordering graph.
    pub fn register_lock(&mut self, id: LockId, name: &str) {
        if !self.locks.iter().any(|n| n.id == id) {
            self.locks.push(LockNode {
                id,
                name: String::from(name),
                held_by: Vec::new(),
            });
        }
    }

    /// Record that `from` must be acquired before `to`.
    ///
    /// This is the lock ordering constraint.  Every acquisition of `to`
    /// while holding `from` creates this edge.
    pub fn record_ordering(&mut self, from: LockId, to: LockId, thread: ThreadId) {
        // Skip if already recorded
        if self.edges.iter().any(|e| e.from == from && e.to == to && e.thread == thread) {
            return;
        }
        self.edges.push(LockEdge { from, to, thread });
    }

    /// Detect cycles in the lock ordering graph.
    ///
    /// A cycle indicates a potential deadlock: threads could acquire
    /// locks in conflicting orders.
    pub fn detect_cycle(&self) -> Option<LockViolation> {
        // Build adjacency list (all threads combined)
        let mut adj: Vec<(LockId, Vec<LockId>)> = self
            .locks
            .iter()
            .map(|n| (n.id, Vec::new()))
            .collect();

        for edge in &self.edges {
            if let Some((_, targets)) = adj.iter_mut().find(|(id, _)| *id == edge.from) {
                if !targets.contains(&edge.to) {
                    targets.push(edge.to);
                }
            }
        }

        // DFS cycle detection
        fn dfs(
            id: LockId,
            adj: &[(LockId, Vec<LockId>)],
            visited: &mut Vec<LockId>,
            stack: &mut Vec<LockId>,
        ) -> Option<Vec<LockId>> {
            if stack.contains(&id) {
                // Found a cycle: extract it
                let pos = stack.iter().position(|x| *x == id).unwrap();
                let cycle = stack[pos..].to_vec();
                return Some(cycle);
            }
            if visited.contains(&id) {
                return None;
            }
            visited.push(id);
            stack.push(id);
            if let Some((_, targets)) = adj.iter().find(|(lid, _)| *lid == id) {
                for t in targets {
                    if let Some(cycle) = dfs(*t, adj, visited, stack) {
                        return Some(cycle);
                    }
                }
            }
            stack.pop();
            None
        }

        let mut visited = Vec::new();
        for (id, _) in &adj {
            if !visited.contains(id) {
                let mut stack = Vec::new();
                if let Some(cycle) = dfs(*id, &adj, &mut visited, &mut stack) {
                    let detail = alloc::format!(
                        "Lock cycle: {}",
                        cycle.iter().map(|c| alloc::format!("{c:?}")).collect::<Vec<_>>().join(" ──► ")
                    );
                    return Some(LockViolation::DeadlockDetected { cycle, detail });
                }
            }
        }
        None
    }

    /// Check mutual exclusion: a lock held by more than one thread
    /// simultaneously is a violation.
    pub fn check_mutual_exclusion(&self) -> Vec<LockViolation> {
        let mut violations = Vec::new();
        for node in &self.locks {
            let unique_holders: Vec<ThreadId> = {
                let mut v: Vec<ThreadId> = node.held_by.clone();
                v.sort();
                v.dedup();
                v
            };
            if unique_holders.len() > 1 {
                violations.push(LockViolation::NoMutualExclusion {
                    lock: node.id,
                    holders: unique_holders,
                });
            }
        }
        violations
    }

    /// Check that threads acquire locks in a consistent global order.
    /// This is a simplified check; a full proof would require static
    /// analysis of the lock hierarchy document.
    pub fn no_deadlock(lock_order: &[LockId]) -> bool {
        // The global lock order must be a total order (or at least a DAG).
        // If every thread acquires locks in increasing LockId order,
        // no cycle is possible.
        for win in lock_order.windows(2) {
            if win[0] >= win[1] {
                return false;
            }
        }
        true
    }

    /// Mutual exclusion predicate.
    pub fn mutual_exclusion(held_by: &[ThreadId]) -> bool {
        let unique: Vec<ThreadId> = {
            let mut v = held_by.to_vec();
            v.sort();
            v.dedup();
            v
        };
        unique.len() <= 1
    }

    /// Progress: every thread waiting for a lock eventually acquires it.
    ///
    /// This is trivially true for `crate::sync::IrqSafeMutex` (the thread spins until
    /// it wins the compare-and-swap) and relies on FIFO wakeup for
    /// `SchedLock` (wake_pipe wakes in insertion order).
    pub fn progress(waiting: &[ThreadId], block_reason: &[BlockReason]) -> bool {
        if waiting.len() != block_reason.len() {
            return false;
        }
        for reason in block_reason {
            match reason {
                BlockReason::HeldByOther(_) => {}  // will be woken when holder drops
                BlockReason::Contended => { /* spinlock: loop until acquire */ }
                BlockReason::OrderViolation => {
                    return false; // ordering violation = no guarantee
                }
            }
        }
        true
    }
}
