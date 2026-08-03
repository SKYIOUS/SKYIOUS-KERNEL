#![allow(dead_code)]

//! Runtime verification harness.
//!
//! The `VerificationRunner` samples invariant checkpoints during kernel
//! execution and reports violations.  It is designed to be zero-cost
//! when the `verification` feature is disabled: the entire module is
//! `#[cfg(feature = "verification")]`.
//!
//! # Usage
//!
//! ```rust,ignore
//! #[cfg(feature = "verification")]
//! {
//!     let mut v = crate::verified::runner::VERIFICATION_RUNNER.lock();
//!     v.checkpoint("scheduler::pick_next", &snapshot);
//!     v.report();
//! }
//! ```

use crate::verified::{Invariant, VerificationFailure, VerificationReport};
use crate::sync::IrqSafeMutex as Mutex;

/// Global verification runner instance.
///
/// Guarded by `crate::sync::IrqSafeMutex` for interrupt-safe access from scheduler
/// checkpoints (which may fire from timer IRQ context).
pub static VERIFICATION_RUNNER: Mutex<VerificationRunner> = Mutex::new(VerificationRunner::new());

/// Runtime proof checker.
///
/// Accumulates checkpoint data and invariant violations.  The report
/// is accessible via `sysctl kernel.verification=1` at runtime.
pub struct VerificationRunner {
    pub enabled: bool,
    pub failures: alloc::vec::Vec<VerificationFailure>,
    pub checkpoints: u64,
    pub violations: u64,
}

impl VerificationRunner {
    /// Create a disabled-by-default runner (enabled via sysctl).
    pub const fn new() -> Self {
        VerificationRunner {
            enabled: false,
            failures: alloc::vec::Vec::new(),
            checkpoints: 0,
            violations: 0,
        }
    }

    /// Enable or disable runtime checking.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Register a checkpoint in a critical code path.
    ///
    /// If the runner is enabled and the state's invariant returns `false`,
    /// the violation is recorded.  Always increments the checkpoint counter
    /// so that coverage statistics are meaningful.
    pub fn checkpoint<S>(
        &mut self,
        name: &str,
        state: &dyn Invariant<State = S>,
    ) {
        self.checkpoints += 1;
        if !self.enabled {
            return;
        }
        if !state.invariant() {
            self.violations += 1;
            self.failures.push(VerificationFailure {
                checkpoint: alloc::string::String::from(name),
                detail: alloc::string::String::from("Invariant violation detected"),
            });
        }
    }

    /// Record an arbitrary verification failure.
    pub fn record_failure(&mut self, checkpoint: &str, detail: &str) {
        self.violations += 1;
        self.failures.push(VerificationFailure {
            checkpoint: alloc::string::String::from(checkpoint),
            detail: alloc::string::String::from(detail),
        });
    }

    /// Produce a report and (optionally) print it via serial.
    pub fn report(&self) -> VerificationReport {
        VerificationReport {
            checkpoints_checked: self.checkpoints,
            failures: self.failures.clone(),
            pass: self.failures.is_empty(),
        }
    }

    /// Serial-print a human-readable summary (for early boot).
    pub fn dump_report(&self) {
        use crate::serial_write;
        serial_write("[VERIFY] ===== Verification Report =====\n");
        serial_write(&alloc::format!("[VERIFY] Checkpoints: {}\n", self.checkpoints));
        serial_write(&alloc::format!("[VERIFY] Violations:  {}\n", self.violations));
        serial_write(&alloc::format!("[VERIFY] Status:     {}\n", if self.failures.is_empty() { "PASS" } else { "FAIL" }));
        for f in &self.failures {
            serial_write(&alloc::format!("[VERIFY]  FAIL: {} — {}\n", f.checkpoint, f.detail));
        }
        serial_write("[VERIFY] ==============================\n");
    }

    /// Reset all accumulated state.
    pub fn reset(&mut self) {
        self.failures.clear();
        self.checkpoints = 0;
        self.violations = 0;
    }
}

/// Convenience macro: run a checkpoint with the global runner.
///
/// Example:
/// ```rust,ignore
/// verify_checkpoint!("scheduler::pick_next", my_state);
/// ```
#[macro_export]
macro_rules! verify_checkpoint {
    ($name:expr, $state:expr) => {
        #[cfg(feature = "verification")]
        {
            let mut __v = $crate::verified::runner::VERIFICATION_RUNNER.lock();
            __v.checkpoint($name, $state);
        }
    };
}
