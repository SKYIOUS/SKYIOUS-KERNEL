//! Open file description for a POSIX message queue.

use crate::ipc::mqueue::queue::{MqAttr, MqQueueHandle};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicI32, Ordering};

const O_NONBLOCK: i32 = 0x800;

/// One open file description: the named queue plus per-open state.
///
/// O_NONBLOCK lives here, not in the queue, so `mq_setattr` on one descriptor
/// does not affect other descriptors (dup shares it, a fresh `mq_open` does
/// not), matching Linux.
pub struct MqObject {
    queue: Arc<MqQueueHandle>,
    flags: AtomicI32,
    read: bool,
    write: bool,
    mode: u32,
    uid: u32,
    gid: u32,
}

impl MqObject {
    /// Create a description for `oflag` on an existing queue.
    pub fn new(queue: Arc<MqQueueHandle>, oflag: i32, mode: u32, uid: u32, gid: u32) -> Self {
        let (read, write) = match oflag & 3 {
            0 => (true, false),
            1 => (false, true),
            _ => (true, true),
        };
        MqObject {
            queue,
            flags: AtomicI32::new(oflag & O_NONBLOCK),
            read,
            write,
            mode,
            uid,
            gid,
        }
    }

    /// Get the underlying queue handle.
    pub fn queue(&self) -> &MqQueueHandle {
        &self.queue
    }

    /// Current O_NONBLOCK flag of this description.
    pub fn flags(&self) -> i32 {
        self.flags.load(Ordering::Acquire)
    }

    /// Replace the O_NONBLOCK flag of this description.
    pub fn set_flags(&self, flags: i32) {
        self.flags.store(flags, Ordering::Release);
    }

    pub fn can_read(&self) -> bool {
        self.read
    }

    pub fn can_write(&self) -> bool {
        self.write
    }

    /// Attributes as seen through this description.
    pub fn get_attr(&self) -> MqAttr {
        self.queue.get_attr(self.flags())
    }

    /// Permission bits of the queue, without the file type.
    pub fn mode(&self) -> u32 {
        self.mode
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }

    pub fn gid(&self) -> u32 {
        self.gid
    }
}
