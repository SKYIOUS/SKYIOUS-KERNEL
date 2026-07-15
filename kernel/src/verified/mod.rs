#![allow(dead_code)]

//! Formal verification infrastructure for critical kernel paths.
//!
//! # Architecture
//!
//! Modeled after seL4's proof stack, this module provides three layers:
//!
//! 1. **Refinement types & contracts** — `Invariant` trait + `SafetyContract`
//!    for runtime-checked pre/post conditions on critical operations.
//! 2. **Domain-specific proof modules** — scheduler pass-invariant checks,
//!    journal state-machine validation, lock-ordering verification.
//! 3. **Proof architecture documentation** — seL4-style refinement arguments
//!    in `kernel/src/verified/proofs/`.
//!
//! # Feature gate
//!
//! All verification code is compiled only under `#[cfg(feature = "verification")]`.
//! When disabled the compiler dead-code-eliminates everything — zero cost.
//!
//! # Current status
//!
//! | Component      | Verification                       | Status           |
//! |----------------|------------------------------------|------------------|
//! | Stride Sched.  | Pass invariant, min-select, bounds | Runtime-checked  |
//! | SkyFS Journal  | State machine, atomicity           | Runtime-checked  |
//! | Interrupts     | Handler state transitions          | Documented       |
//! | Locks          | Ordering, deadlock prevention      | Documented       |

#[cfg(feature = "verification")]
pub mod scheduler;
#[cfg(feature = "verification")]
pub mod journal;
#[cfg(feature = "verification")]
pub mod concurrency;
#[cfg(feature = "verification")]
pub mod runner;

/// Runtime-checked invariant trait (refinement-type analogue).
///
/// Every type that carries a correctness condition implements this
/// so the verification runner can sample it at checkpoints.
pub trait Invariant {
    type State;
    fn invariant(&self) -> bool;
}

/// Two-state safety contract (pre/post condition pair).
///
/// `Precondition` — snapshot taken before the operation.
/// `Postcondition` — snapshot taken after the operation.
/// `Error` — domain-specific error type.
pub trait SafetyContract {
    type Precondition;
    type Postcondition;
    type Error;

    fn precondition(&self, state: &Self::Precondition) -> bool;
    fn postcondition(
        &self,
        old_state: &Self::Precondition,
        new_state: &Self::Postcondition,
    ) -> Result<(), Self::Error>;
}

/// Verification failure detail, reported by the runner.
#[derive(Debug, Clone)]
pub struct VerificationFailure {
    pub checkpoint: alloc::string::String,
    pub detail: alloc::string::String,
}

/// Summary report produced at runtime.
#[derive(Debug, Clone)]
pub struct VerificationReport {
    pub checkpoints_checked: u64,
    pub failures: alloc::vec::Vec<VerificationFailure>,
    pub pass: bool,
}
