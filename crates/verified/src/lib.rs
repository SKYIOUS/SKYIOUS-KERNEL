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
//!    in `crates/verified/proofs/`.

#![no_std]

extern crate alloc;

pub mod concurrency;
pub mod journal;
pub mod runner;
pub mod scheduler;

pub use runner::VERIFICATION_RUNNER;

#[cfg(all(not(test), target_os = "none"))]
extern "Rust" {
    fn vahi_kernel_serial_write(msg: &str);
}

/// Serial line output forwarding helper.
#[cfg(all(not(test), target_os = "none"))]
pub fn serial_write(msg: &str) {
    unsafe { vahi_kernel_serial_write(msg) }
}

#[cfg(not(all(not(test), target_os = "none")))]
pub fn serial_write(_msg: &str) {}

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

#[cfg(test)]
mod tests {
    use super::*;
    use concurrency::{LockId, LockOrderVerifier, ThreadId};
    use journal::{JournalEvent, JournalState, JournalStateMachine};
    use scheduler::{check_schedule_correctness, schedule_contract, SchedSnapshot, Thread};

    struct TestState(bool);
    impl Invariant for TestState {
        type State = bool;
        fn invariant(&self) -> bool {
            self.0
        }
    }

    #[test]
    fn test_journal_state_machine_valid_flow() {
        let mut sm = JournalStateMachine::new();
        assert_eq!(sm.state, JournalState::Idle);

        assert!(sm.apply(JournalEvent::BeginTxn).is_ok());
        assert_eq!(sm.state, JournalState::Collecting);

        assert!(sm.apply(JournalEvent::CommitTxn).is_ok());
        assert_eq!(sm.state, JournalState::Committing);

        assert!(sm.apply(JournalEvent::TxnPersisted).is_ok());
        assert_eq!(sm.state, JournalState::Idle);
    }

    #[test]
    fn test_journal_state_machine_crash_and_recovery() {
        let mut sm = JournalStateMachine::new();
        sm.apply(JournalEvent::BeginTxn).unwrap();
        sm.apply(JournalEvent::CommitTxn).unwrap();

        assert!(sm.apply(JournalEvent::Crash).is_ok());
        assert_eq!(sm.state, JournalState::Recovering);

        assert!(sm.apply(JournalEvent::RecoveryComplete).is_ok());
        assert_eq!(sm.state, JournalState::Idle);
        assert_eq!(sm.replayed_txns.len(), 1);
    }

    #[test]
    fn test_lock_order_cycle_detection() {
        let mut verifier = LockOrderVerifier::new();
        let l1 = LockId(1);
        let l2 = LockId(2);
        let t1 = ThreadId(10);
        let t2 = ThreadId(11);

        verifier.register_lock(l1, "Lock1");
        verifier.register_lock(l2, "Lock2");

        verifier.record_ordering(l1, l2, t1);
        verifier.record_ordering(l2, l1, t2);

        assert!(verifier.detect_cycle().is_some());
    }

    #[test]
    fn test_scheduler_correctness_checks() {
        let t1 = Thread {
            pass: 100,
            tickets: 10,
        };
        let t2 = Thread {
            pass: 200,
            tickets: 10,
        };
        let threads = [&t1, &t2];

        let snap = SchedSnapshot {
            threads: &threads,
            selected_idx: 0,
            elapsed_ticks: 1,
        };

        assert!(check_schedule_correctness(&snap).is_ok());

        let contract_res = schedule_contract(&threads, 1);
        assert_eq!(contract_res.unwrap(), 0);
    }

    #[test]
    fn test_verification_runner_failures() {
        let mut runner = runner::VerificationRunner::new();
        runner.set_enabled(true);

        let valid = TestState(true);
        let invalid = TestState(false);

        runner.checkpoint("check_valid", &valid);
        assert_eq!(runner.checkpoints, 1);
        assert_eq!(runner.violations, 0);

        runner.checkpoint("check_invalid", &invalid);
        assert_eq!(runner.checkpoints, 2);
        assert_eq!(runner.violations, 1);

        let rep = runner.report();
        assert_eq!(rep.checkpoints_checked, 2);
        assert!(!rep.pass);
        assert_eq!(rep.failures.len(), 1);
    }
}
