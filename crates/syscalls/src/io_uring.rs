//! io_uring stub for crate extraction.
//! The real implementation lives in `kernel/src/syscalls/io_uring.rs`.

extern crate alloc;

/// Stub IoUringInstance for crate extraction.
#[derive(Debug, Clone, Default)]
pub struct IoUringInstance {}

impl IoUringInstance {
    pub fn new() -> Self {
        IoUringInstance {}
    }
}
