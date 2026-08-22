use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};
use crate::sync::IrqSafeMutex as Mutex;

pub mod handle;
pub mod namespace;
pub mod security;
pub mod net_integration;

/// Unique identifier for each category of kernel object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObjectTypeId(pub u16);

pub const TYPE_FILE: ObjectTypeId = ObjectTypeId(1);
pub const TYPE_DIR: ObjectTypeId = ObjectTypeId(2);
pub const TYPE_SYMLINK: ObjectTypeId = ObjectTypeId(3);
pub const TYPE_DEVICE: ObjectTypeId = ObjectTypeId(4);
pub const TYPE_PIPE: ObjectTypeId = ObjectTypeId(5);
pub const TYPE_SOCKET: ObjectTypeId = ObjectTypeId(6);
pub const TYPE_PTY_MASTER: ObjectTypeId = ObjectTypeId(7);
pub const TYPE_PTY_SLAVE: ObjectTypeId = ObjectTypeId(8);
pub const TYPE_PROCESS: ObjectTypeId = ObjectTypeId(9);
pub const TYPE_THREAD: ObjectTypeId = ObjectTypeId(10);
pub const TYPE_MUTEX: ObjectTypeId = ObjectTypeId(11);
pub const TYPE_SEMAPHORE: ObjectTypeId = ObjectTypeId(12);
pub const TYPE_TIMER: ObjectTypeId = ObjectTypeId(13);
pub const TYPE_EVENT: ObjectTypeId = ObjectTypeId(14);

/// Reference-counted header embedded in every kernel object.
pub struct ObjectHeader {
    pub ref_count: AtomicU32,
    pub object_type: ObjectTypeId,
    pub name: Mutex<Option<alloc::string::String>>,
    pub security: Mutex<security::SecurityDescriptor>,
}

impl ObjectHeader {
    pub fn new(object_type: ObjectTypeId, sec: security::SecurityDescriptor) -> Self {
        ObjectHeader {
            ref_count: AtomicU32::new(1),
            object_type,
            name: Mutex::new(None),
            security: Mutex::new(sec),
        }
    }

    pub fn ref_inc(&self) -> u32 {
        self.ref_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn ref_dec(&self) -> u32 {
        self.ref_count.fetch_sub(1, Ordering::Relaxed) - 1
    }

    pub fn ref_current(&self) -> u32 {
        self.ref_count.load(Ordering::Relaxed)
    }
}

/// Unified trait for every kernel resource.
/// Default methods return `Err(())` — implementors override what they support.
#[allow(clippy::result_unit_err)]
pub trait KernelObject: Send + Sync {
    fn header(&self) -> &ObjectHeader;

    fn type_id(&self) -> ObjectTypeId { self.header().object_type }

    // ── File-like I/O ──────────────────────────────────────────────
    fn read(&self, _offset: &mut u64, _buf: &mut [u8]) -> Result<usize, ()> { Err(()) }
    fn write(&self, _offset: &mut u64, _buf: &[u8]) -> Result<usize, ()> { Err(()) }
    fn ioctl(&self, _request: u64, _argp: *mut u8) -> Result<u64, ()> { Err(()) }
    fn stat(&self) -> Result<crate::vfs::Stat, ()> { Err(()) }
    fn truncate(&self, _len: i64) -> Result<(), ()> { Err(()) }
    fn poll_readable(&self) -> bool { false }
    fn poll_writable(&self) -> bool { false }

    // ── Socket-like operations ─────────────────────────────────────
    fn socket_bind(&self, _addr: &[u8]) -> Result<(), ()> { Err(()) }
    fn socket_connect(&self, _addr: &[u8]) -> Result<(), ()> { Err(()) }
    fn socket_listen(&self, _backlog: usize) -> Result<(), ()> { Err(()) }
    fn socket_accept(&self) -> Result<Arc<dyn KernelObject>, ()> { Err(()) }
    fn socket_peer_name(&self) -> Result<alloc::vec::Vec<u8>, ()> { Err(()) }
    fn socket_local_name(&self) -> Result<alloc::vec::Vec<u8>, ()> { Err(()) }

    // ── Metadata ──────────────────────────────────────────────────────────────────
    fn type_name(&self) -> &'static str { "KernelObject" }
    fn query_name(&self) -> Option<alloc::string::String> { None }
    fn set_name(&self, _name: &str) {}

    // ── Handle lifecycle hooks ─────────────────────────────────────────────────
    fn on_handle_create(&self) {}
    fn on_handle_close(&self) {}

    // ── Lifecycle ──────────────────────────────────────────────────
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

// PYTHON_WROTE_THIS
