use alloc::sync::Arc;

pub mod handle;
pub mod namespace;
pub mod net_integration;
pub mod security;

pub use vahi_objects::ObjectHeader;
pub use vahi_objects::ObjectTypeId;
pub use vahi_objects::{
    TYPE_DEVICE, TYPE_DIR, TYPE_EVENT, TYPE_FILE, TYPE_MUTEX, TYPE_PIPE, TYPE_PROCESS,
    TYPE_PTY_MASTER, TYPE_PTY_SLAVE, TYPE_SEMAPHORE, TYPE_SOCKET, TYPE_SYMLINK, TYPE_THREAD,
    TYPE_TIMER,
};

// Re-export the crate's KernelObject trait as the canonical type.
pub use vahi_objects::KernelObject;

/// Extension trait for kernel-only operations (socket, lifecycle).
/// Implemented for `dyn KernelObject` via blanket impl below.
pub trait KernelObjectExt {
    fn socket_bind(&self, _addr: &[u8]) -> Result<(), ()> {
        Err(())
    }
    fn socket_connect(&self, _addr: &[u8]) -> Result<(), ()> {
        Err(())
    }
    fn socket_listen(&self, _backlog: usize) -> Result<(), ()> {
        Err(())
    }
    fn socket_accept(&self) -> Result<Arc<dyn KernelObject>, ()> {
        Err(())
    }
    fn socket_peer_name(&self) -> Result<alloc::vec::Vec<u8>, ()> {
        Err(())
    }
    fn socket_local_name(&self) -> Result<alloc::vec::Vec<u8>, ()> {
        Err(())
    }
    fn on_handle_create(&self) {}
    fn on_handle_close(&self) {}
    fn on_close(&self) {}
}

/// Snapshot the current process's effective credentials.
/// Returns a zero-filled struct when no process is active.
pub fn current_credentials() -> security::Credentials {
    let lock = crate::task::process::CURRENT_PROCESS.lock();
    match lock.as_ref() {
        Some(p) => {
            let mut caps = security::Credentials::new();
            let creds = p.creds.lock();
            caps.euid = creds.euid;
            caps.egid = creds.egid;
            caps.uid = creds.uid;
            caps.gid = creds.gid;
            caps.fsuid = creds.fsuid;
            caps.fsgid = creds.fsgid;
            caps.cap_effective = creds.cap_effective;
            caps
        }
        None => security::Credentials::new(),
    }
}
