//! T-00 harness validation tests (deterministic, no environment dependencies).
//!
//! These tests validate the test infrastructure itself: the kernel selftest
//! runner, the TAP serial protocol, the QEMU isa-debug-exit path, and the host
//! runner's result classification. They exercise no other kernel subsystem.
//!
//! The suite variant is chosen at build time with `VAHI_SELFTEST_MODE`:
//!   (unset or "pass")    `harness::always_pass`         — suite completes, QEMU exits 0x10 → PASS
//!   "fail"               `harness::controlled_failure`   — returns Err   → FAIL  → QEMU panic exit
//!   "timeout"            `harness::controlled_timeout`   — spins forever → TIMEOUT (host wall clock)
//!   "panic"              `harness::controlled_panic`     — panics        → PANIC → QEMU exit 0x11
//!
//! Only the default variant is registered in normal builds; the fail/timeout/
//! panic variants exist solely to prove the harness reports non-PASS results,
//! and are never part of the regular suite.

/// Test A: deterministic pass. Exercises the register/run/protocol path with
/// real computation (memory round-trip through `alloc`), not just printing.
fn test_always_pass() -> Result<(), &'static str> {
    let mut probe = alloc::vec![0u8; 256];
    for (i, b) in probe.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
    let sum: u32 = probe.iter().map(|&b| b as u32).sum();
    let expected: u32 = (0..256).map(|i| (i % 251) as u32).sum();
    if sum != expected {
        return Err("harness memory round-trip checksum mismatch");
    }
    Ok(())
}

/// Test B: controlled failure. Returns Err so the runner prints a `not ok`
/// TAP line; run_all() then panics, which exits QEMU with the panic code.
fn test_controlled_failure() -> Result<(), &'static str> {
    Err("T-00 controlled failure: harness must report FAIL and non-zero host exit")
}

/// Test C: controlled hang. Never returns, so no TAP completion is printed and
/// the host runner's wall-clock timeout classifies the run as TIMEOUT.
fn test_controlled_timeout() -> Result<(), &'static str> {
    // Spin deliberately; a 64-bit counter at ~10^9 iterations/second takes
    // centuries to wrap, so this cannot terminate on its own. The panic
    // handler is untouched: only the host timeout ends this run.
    let mut sink = 0u64;
    loop {
        sink = sink.wrapping_add(1);
        core::hint::spin_loop();
    }
}

/// Test D: controlled panic. The panic handler emits the machine-readable
/// `Bail out! KERNEL PANIC` marker and exits QEMU with code 0x11, which the
/// host runner maps to PANIC — proving panics are reported as failures.
fn test_controlled_panic() -> Result<(), &'static str> {
    panic!("T-00 controlled panic: harness must report PANIC and non-zero host exit");
}

#[cfg(feature = "self_test")]
pub fn register() {
    use crate::selftest;
    // The panic demo aborts the whole run at the point it executes, so it is
    // only ever registered alone (mode = "panic").
    match option_env!("VAHI_SELFTEST_MODE") {
        Some("fail") => {
            selftest::register("harness::controlled_failure", test_controlled_failure);
        }
        Some("timeout") => {
            selftest::register("harness::controlled_timeout", test_controlled_timeout);
        }
        Some("panic") => {
            selftest::register("harness::controlled_panic", test_controlled_panic);
        }
        // Default build (and mode = "pass"): the deterministic-pass demo that
        // ships in the regular self_test suite.
        _ => {
            selftest::register("harness::always_pass", test_always_pass);
        }
    }
}
