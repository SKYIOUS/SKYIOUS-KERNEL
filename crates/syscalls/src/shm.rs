//! shm stub for crate extraction.
//! The real implementation lives in `kernel/src/syscalls/shm.rs`.

extern crate alloc;

/// Stub: detach all shared memory segments for a process.
#[allow(dead_code)]
pub fn shm_detach_all(_pid: u64) {}
