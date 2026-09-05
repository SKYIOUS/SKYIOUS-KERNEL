//! process_lifecycle stub for crate extraction.
//! The real implementation lives in `kernel/src/syscalls/process_lifecycle.rs`.

extern crate alloc;

/// Stub: close all file descriptors for a process.
#[allow(dead_code)]
pub fn process_close_all_fds(_pid: u64) {}
