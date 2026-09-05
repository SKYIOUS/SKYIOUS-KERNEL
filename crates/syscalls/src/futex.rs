//! futex stub for crate extraction.
//! The real implementation lives in `kernel/src/syscalls/futex.rs`.

extern crate alloc;

/// Stub: wake all futex threads for a given process.
#[allow(dead_code)]
pub fn wake_process_futex_threads(_pid: u64) {}

/// Stub: wake all blocked threads for a given process.
#[allow(dead_code)]
pub fn wake_process_blocked_threads(_pid: u64) {}
