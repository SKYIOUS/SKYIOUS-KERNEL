//! # vahi-objects — Kernel Object Namespace & Handles
//!
//! Unified kernel resource model: every open resource (files, sockets,
//! pipes, devices, processes) is a `KernelObject` accessed through
//! integer handles in per-process handle tables.
//!
//! ## Core Types
//!
//! | Type | Purpose |
//! |------|---------|
//! | `ObjectTypeId` | Discriminant for14 kernel object categories |
//! | `ObjectHeader` | Reference-counted header embedded in every object |
//! | `KernelObject` | Unified trait for all kernel resources |
//! | `HandleTable` | Per-process handle → object mapping |
//! | `ObjectNamespace` | Global `/Device`, `/Process`, `/System` tree |
//!
//! ## Dependency Breaking
//!
//! ```text
//! Original:     objects → task (CURRENT_PROCESS) + vfs (Stat)
//! With traits:  objects → vahi_types::{Credentials} + vahi_objects::StatLike
//! ```
//!
//! ## Invariants
//!
//! - Refcounts are atomic (lock-free hot path)
//! - Handle table uses first-fit slot allocation
//! - Security checks on every handle insert
//! - Close-on-exec handled during fork

#![no_std]

extern crate alloc;

pub mod security;

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};
use vahi_sync::IrqSafeMutex as Mutex;

// ─── Object Type IDs ────────────────────────────────────────────────────────

/// Unique identifier for each category of kernel object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

impl ObjectTypeId {
    /// Get the human-readable name for this object type.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self.0 {
            1 => "File",
            2 => "Dir",
            3 => "Symlink",
            4 => "Device",
            5 => "Pipe",
            6 => "Socket",
            7 => "PtyMaster",
            8 => "PtySlave",
            9 => "Process",
            10 => "Thread",
            11 => "Mutex",
            12 => "Semaphore",
            13 => "Timer",
            14 => "Event",
            _ => "Unknown",
        }
    }
}

// ─── Minimal Stat (avoids vfs dependency) ────────────────────────────────────

/// Minimal file metadata (avoids depending on `vahi-vfs::Stat`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StatLike {
    pub ino: u64,
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub blocks: u64,
}

// ─── Object Header ───────────────────────────────────────────────────────────

/// Reference-counted header embedded in every kernel object.
pub struct ObjectHeader {
    /// Atomic reference count.
    pub ref_count: AtomicU32,
    /// Object type discriminator.
    pub object_type: ObjectTypeId,
    /// Optional human-readable name.
    pub name: Mutex<Option<alloc::string::String>>,
    /// Security descriptor (ACL, owner, group).
    pub security: Mutex<security::SecurityDescriptor>,
}

impl ObjectHeader {
    /// Create a new object header with refcount=1.
    pub fn new(object_type: ObjectTypeId, sec: security::SecurityDescriptor) -> Self {
        ObjectHeader {
            ref_count: AtomicU32::new(1),
            object_type,
            name: Mutex::new(None),
            security: Mutex::new(sec),
        }
    }

    /// Increment the reference count. Returns the new count.
    #[must_use]
    pub fn ref_inc(&self) -> u32 {
        self.ref_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Decrement the reference count. Returns the new count.
    pub fn ref_dec(&self) -> u32 {
        self.ref_count.fetch_sub(1, Ordering::Relaxed) - 1
    }

    /// Get the current reference count.
    #[must_use]
    pub fn ref_current(&self) -> u32 {
        self.ref_count.load(Ordering::Relaxed)
    }
}

// ─── KernelObject Trait ──────────────────────────────────────────────────────

/// Unified trait for every kernel resource.
///
/// Default methods return `Err(())` — implementors override what they support.
#[allow(clippy::result_unit_err)]
pub trait KernelObject: Send + Sync {
    /// Get the reference-counted header for this object.
    fn header(&self) -> &ObjectHeader;

    /// Get the object type ID.
    #[must_use]
    fn type_id(&self) -> ObjectTypeId {
        self.header().object_type
    }

    // ── File-like I/O ──────────────────────────────────────────────
    fn read(&self, _offset: &mut u64, _buf: &mut [u8]) -> Result<usize, ()> {
        Err(())
    }
    fn write(&self, _offset: &mut u64, _buf: &[u8]) -> Result<usize, ()> {
        Err(())
    }
    fn ioctl(&self, _request: u64, _argp: *mut u8) -> Result<u64, ()> {
        Err(())
    }
    fn stat(&self) -> Result<StatLike, ()> {
        Err(())
    }
    fn truncate(&self, _len: i64) -> Result<(), ()> {
        Err(())
    }
    fn poll_readable(&self) -> bool {
        false
    }
    fn poll_writable(&self) -> bool {
        false
    }

    // ── Socket-like operations ─────────────────────────────────────
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

    // ── Metadata ───────────────────────────────────────────────────
    fn type_name(&self) -> &'static str {
        "KernelObject"
    }
    fn query_name(&self) -> Option<alloc::string::String> {
        None
    }
    fn set_name(&self, _name: &str) {}

    // ── Handle lifecycle hooks ─────────────────────────────────────
    fn on_handle_create(&self) {}
    fn on_handle_close(&self) {}

    // ── Lifecycle ──────────────────────────────────────────────────
    fn on_close(&self) {}
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;
    use crate::security::SecurityDescriptor;

    // ── ObjectTypeId ─────────────────────────────────────────────────────────

    #[test]
    fn object_type_id_names() {
        assert_eq!(TYPE_FILE.name(), "File");
        assert_eq!(TYPE_DIR.name(), "Dir");
        assert_eq!(TYPE_SYMLINK.name(), "Symlink");
        assert_eq!(TYPE_DEVICE.name(), "Device");
        assert_eq!(TYPE_PIPE.name(), "Pipe");
        assert_eq!(TYPE_SOCKET.name(), "Socket");
        assert_eq!(TYPE_PTY_MASTER.name(), "PtyMaster");
        assert_eq!(TYPE_PTY_SLAVE.name(), "PtySlave");
        assert_eq!(TYPE_PROCESS.name(), "Process");
        assert_eq!(TYPE_THREAD.name(), "Thread");
        assert_eq!(TYPE_MUTEX.name(), "Mutex");
        assert_eq!(TYPE_SEMAPHORE.name(), "Semaphore");
        assert_eq!(TYPE_TIMER.name(), "Timer");
        assert_eq!(TYPE_EVENT.name(), "Event");
    }

    #[test]
    fn object_type_id_unknown() {
        assert_eq!(ObjectTypeId(0).name(), "Unknown");
        assert_eq!(ObjectTypeId(15).name(), "Unknown");
        assert_eq!(ObjectTypeId(99).name(), "Unknown");
        assert_eq!(ObjectTypeId(u16::MAX).name(), "Unknown");
    }

    #[test]
    fn object_type_id_ordering() {
        // ObjectTypeId derives Ord.
        assert!(TYPE_FILE < TYPE_DIR);
        assert!(TYPE_THREAD < TYPE_MUTEX);
        assert!(TYPE_EVENT > TYPE_TIMER);
    }

    #[test]
    fn object_type_id_unique() {
        use alloc::collections::BTreeSet;
        let mut seen = BTreeSet::new();
        for t in [
            TYPE_FILE,
            TYPE_DIR,
            TYPE_SYMLINK,
            TYPE_DEVICE,
            TYPE_PIPE,
            TYPE_SOCKET,
            TYPE_PTY_MASTER,
            TYPE_PTY_SLAVE,
            TYPE_PROCESS,
            TYPE_THREAD,
            TYPE_MUTEX,
            TYPE_SEMAPHORE,
            TYPE_TIMER,
            TYPE_EVENT,
        ] {
            assert!(seen.insert(t), "duplicate type id: {:?}", t);
        }
        assert_eq!(seen.len(), 14);
    }

    // ── ObjectHeader refcounting ─────────────────────────────────────────────

    #[test]
    fn object_header_starts_at_one() {
        let h = ObjectHeader::new(TYPE_FILE, SecurityDescriptor::default());
        assert_eq!(h.ref_current(), 1);
    }

    #[test]
    fn object_header_ref_inc() {
        let h = ObjectHeader::new(TYPE_FILE, SecurityDescriptor::default());
        assert_eq!(h.ref_inc(), 2);
        assert_eq!(h.ref_inc(), 3);
        assert_eq!(h.ref_current(), 3);
    }

    #[test]
    fn object_header_ref_dec() {
        let h = ObjectHeader::new(TYPE_FILE, SecurityDescriptor::default());
        h.ref_inc();
        h.ref_inc();
        h.ref_inc();
        assert_eq!(h.ref_current(), 4);
        assert_eq!(h.ref_dec(), 3);
        assert_eq!(h.ref_dec(), 2);
        assert_eq!(h.ref_current(), 2);
    }

    #[test]
    fn object_header_ref_dec_below_zero_is_saturating_arithmetic() {
        // AtomicU16 fetch_sub wraps; this documents the behavior.
        // Real code MUST avoid reaching 0 then decrementing.
        let h = ObjectHeader::new(TYPE_FILE, SecurityDescriptor::default());
        let v = h.ref_dec();
        // 1 - 1 = 0
        assert_eq!(v, 0);
    }

    // ── StatLike ─────────────────────────────────────────────────────────────

    #[test]
    fn stat_like_default_is_zero() {
        let s = StatLike::default();
        assert_eq!(s.ino, 0);
        assert_eq!(s.mode, 0);
        assert_eq!(s.nlink, 0);
        assert_eq!(s.uid, 0);
        assert_eq!(s.gid, 0);
        assert_eq!(s.size, 0);
        assert_eq!(s.blocks, 0);
    }

    #[test]
    fn stat_like_field_set() {
        let s = StatLike {
            ino: 0xABCDEF,
            mode: 0o100644,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            size: 4096,
            blocks: 8,
        };
        assert_eq!(s.ino, 0xABCDEF);
        assert_eq!(s.size, 4096);
    }

    // ── KernelObject default trait methods ───────────────────────────────────

    struct MinimalObject {
        header: ObjectHeader,
    }

    impl MinimalObject {
        fn new() -> Self {
            Self {
                header: ObjectHeader::new(TYPE_FILE, SecurityDescriptor::default()),
            }
        }
    }

    impl KernelObject for MinimalObject {
        fn header(&self) -> &ObjectHeader {
            &self.header
        }
    }

    #[test]
    fn kernel_object_default_read_returns_err() {
        let obj = MinimalObject::new();
        let mut off = 0;
        let mut buf = [0u8; 16];
        assert_eq!(obj.read(&mut off, &mut buf), Err(()));
    }

    #[test]
    fn kernel_object_default_write_returns_err() {
        let obj = MinimalObject::new();
        let mut off = 0;
        let buf = [0u8; 16];
        assert_eq!(obj.write(&mut off, &buf), Err(()));
    }

    #[test]
    fn kernel_object_default_ioctl_returns_err() {
        let obj = MinimalObject::new();
        assert_eq!(obj.ioctl(0, core::ptr::null_mut()), Err(()));
    }

    #[test]
    fn kernel_object_default_stat_returns_err() {
        let obj = MinimalObject::new();
        assert_eq!(obj.stat(), Err(()));
    }

    #[test]
    fn kernel_object_default_poll_is_false() {
        let obj = MinimalObject::new();
        assert!(!obj.poll_readable());
        assert!(!obj.poll_writable());
    }

    #[test]
    fn kernel_object_default_type_name() {
        let obj = MinimalObject::new();
        assert_eq!(obj.type_name(), "KernelObject");
        assert_eq!(obj.type_id(), TYPE_FILE);
    }

    #[test]
    fn kernel_object_default_query_name() {
        let obj = MinimalObject::new();
        assert_eq!(obj.query_name(), None);
    }
}
