//! inotify stub for crate extraction.
//! The real implementation lives in `kernel/src/syscalls/inotify.rs`.

extern crate alloc;

/// Stub InotifyInstance for crate extraction.
#[derive(Debug, Clone, Default)]
pub struct InotifyInstance {}

impl InotifyInstance {
    pub fn new() -> Self {
        InotifyInstance {}
    }
}
