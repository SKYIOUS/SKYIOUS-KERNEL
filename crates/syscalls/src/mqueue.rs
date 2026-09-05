//! mqueue stub for crate extraction.
//! The real implementation lives in `kernel/src/syscalls/mqueue.rs`.

extern crate alloc;

/// Stub: close all message queues for a process.
#[allow(dead_code)]
pub fn mq_close_all(_pid: u64) {}
