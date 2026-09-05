//! timerfd stub for crate extraction.
//! The real implementation lives in `kernel/src/syscalls/timerfd.rs`.

extern crate alloc;

/// Stub: check all timerfds for expiry.
#[allow(dead_code)]
pub fn check_timerfds() {}
