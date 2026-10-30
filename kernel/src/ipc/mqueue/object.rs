//! Open file description for a POSIX message queue.

use crate::ipc::mqueue::queue::{MqAttr, MqQueueHandle};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

const O_NONBLOCK: i32 = 0x800;

/// One open file description: the named queue plus per-open state.
///
/// O_NONBLOCK lives here, not in the queue, so `mq_setattr` on one descriptor
/// does not affect other descriptors (dup shares it, a fresh `mq_open` does
/// not), matching Linux.
pub struct MqObject {
    queue: Arc<MqQueueHandle>,
    flags: AtomicI32,
    /// `read(2)` position; shared by dup, independent per `mq_open`, as the
    /// file position is per open file description.
    pos: AtomicU64,
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
            pos: AtomicU64::new(0),
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

    /// Next `count` bytes of the status line `read(2)` reports, advancing the
    /// file position. Empty at EOF, matching `simple_read_from_buffer`.
    pub fn read_line(&self, count: usize) -> Vec<u8> {
        let line = self.queue.describe_line().into_bytes();
        let pos = self.pos.load(Ordering::Acquire) as usize;
        if pos >= line.len() {
            return Vec::new();
        }
        let end = core::cmp::min(line.len(), pos.saturating_add(count));
        self.pos.store(end as u64, Ordering::Release);
        line[pos..end].to_vec()
    }
}
