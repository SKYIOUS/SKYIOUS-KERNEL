//! Message queue core data structure.

use crate::sync::IrqSafeMutex as Mutex;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

const O_NONBLOCK: i32 = 0x800;
const MQ_PRIO_MAX: u32 = 32768;

/// A single message in the queue.
#[derive(Clone)]
struct MqMsg {
    data: Vec<u8>,
    prio: u32,
}

/// POSIX message queue attributes.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct MqAttr {
    pub mq_flags: i64,
    pub mq_maxmsg: i64,
    pub mq_msgsize: i64,
    pub mq_curmsgs: i64,
    pub __reserved: [i64; 4],
}

impl MqAttr {
    /// Create default attributes (Linux defaults).
    pub fn default_attrs() -> Self {
        MqAttr {
            mq_flags: 0,
            mq_maxmsg: 10,
            mq_msgsize: 8192,
            mq_curmsgs: 0,
            __reserved: [0; 4],
        }
    }
}

/// Message queue state.
pub struct MqQueue {
    /// Messages sorted by priority (highest first).
    msgs: VecDeque<MqMsg>,
    /// Maximum number of messages in queue.
    max_msgs: usize,
    /// Maximum size of a single message.
    max_msg_size: usize,
    /// Whether the queue has been unlinked.
    deleted: bool,
    /// Open file description flags (O_NONBLOCK, O_CLOEXEC).
    flags: i32,
}

impl MqQueue {
    /// Create a new message queue with the given attributes.
    pub fn new(attr: Option<MqAttr>, flags: i32) -> Self {
        let attr = attr.unwrap_or_else(MqAttr::default_attrs);
        MqQueue {
            msgs: VecDeque::new(),
            max_msgs: attr.mq_maxmsg as usize,
            max_msg_size: attr.mq_msgsize as usize,
            deleted: false,
            flags,
        }
    }

    /// Get current queue attributes.
    pub fn get_attr(&self) -> MqAttr {
        MqAttr {
            mq_flags: self.flags as i64,
            mq_maxmsg: self.max_msgs as i64,
            mq_msgsize: self.max_msg_size as i64,
            mq_curmsgs: self.msgs.len() as i64,
            __reserved: [0; 4],
        }
    }

    /// Set queue attributes (only mq_flags can be changed).
    /// Returns the attributes that were in effect before the call (POSIX).
    pub fn set_attr(&mut self, attr: &MqAttr) -> MqAttr {
        let old = self.get_attr();
        self.flags = attr.mq_flags as i32;
        old
    }

    /// Check if queue is full.
    pub fn is_full(&self) -> bool {
        self.msgs.len() >= self.max_msgs
    }

    /// Check if queue is empty.
    pub fn is_empty(&self) -> bool {
        self.msgs.is_empty()
    }

    /// Get number of messages in queue.
    pub fn len(&self) -> usize {
        self.msgs.len()
    }

    /// Mark queue as deleted.
    pub fn mark_deleted(&mut self) {
        self.deleted = true;
    }

    /// Check if queue is deleted.
    pub fn is_deleted(&self) -> bool {
        self.deleted
    }

    /// Get the flags.
    pub fn flags(&self) -> i32 {
        self.flags
    }

    /// Send a message to the queue.
    /// Returns Err(()) if queue is full and O_NONBLOCK is set.
    pub fn send(&mut self, data: Vec<u8>, prio: u32) -> Result<(), ()> {
        if prio >= MQ_PRIO_MAX {
            return Err(());
        }
        if data.len() > self.max_msg_size {
            return Err(());
        }
        if self.is_full() {
            if (self.flags & O_NONBLOCK) != 0 {
                return Err(());
            }
            // Blocking send: caller must handle retry
            return Err(());
        }
        let msg = MqMsg { data, prio };
        // Insert in priority order (highest priority first)
        let pos = self
            .msgs
            .iter()
            .position(|m| m.prio < prio)
            .unwrap_or(self.msgs.len());
        self.msgs.insert(pos, msg);
        Ok(())
    }

    /// Receive a message from the queue.
    /// Returns the message data and priority, or Err(()) if empty and O_NONBLOCK.
    pub fn receive(&mut self, buf: &mut [u8]) -> Result<(usize, u32), ()> {
        if self.is_empty() {
            if (self.flags & O_NONBLOCK) != 0 {
                return Err(());
            }
            // Blocking receive: caller must handle retry
            return Err(());
        }
        let msg = match self.msgs.pop_front() {
            Some(m) => m,
            // Unreachable after the `is_empty` check; no panic on the syscall path.
            None => return Err(()),
        };
        if buf.len() < msg.data.len() {
            return Err(());
        }
        buf[..msg.data.len()].copy_from_slice(&msg.data);
        Ok((msg.data.len(), msg.prio))
    }

    /// Maximum message size.
    pub fn max_msg_size(&self) -> usize {
        self.max_msg_size
    }

    /// Maximum number of messages.
    pub fn max_msgs(&self) -> usize {
        self.max_msgs
    }
}

/// Thread-safe message queue wrapper.
pub struct MqQueueHandle {
    inner: Mutex<MqQueue>,
}

impl MqQueueHandle {
    pub fn new(attr: Option<MqAttr>, flags: i32) -> Self {
        MqQueueHandle {
            inner: Mutex::new(MqQueue::new(attr, flags)),
        }
    }

    pub fn get_attr(&self) -> MqAttr {
        self.inner.lock().get_attr()
    }

    pub fn set_attr(&self, attr: &MqAttr) -> MqAttr {
        self.inner.lock().set_attr(attr)
    }

    pub fn is_full(&self) -> bool {
        self.inner.lock().is_full()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    pub fn mark_deleted(&self) {
        self.inner.lock().mark_deleted();
    }

    pub fn is_deleted(&self) -> bool {
        self.inner.lock().is_deleted()
    }

    pub fn flags(&self) -> i32 {
        self.inner.lock().flags()
    }

    pub fn send(&self, data: Vec<u8>, prio: u32) -> Result<(), ()> {
        self.inner.lock().send(data, prio)
    }

    pub fn receive(&self, buf: &mut [u8]) -> Result<(usize, u32), ()> {
        self.inner.lock().receive(buf)
    }

    pub fn max_msg_size(&self) -> usize {
        self.inner.lock().max_msg_size()
    }

    pub fn max_msgs(&self) -> usize {
        self.inner.lock().max_msgs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_basic() {
        let q = MqQueue::new(None, 0);
        assert_eq!(q.max_msgs(), 10);
        assert_eq!(q.max_msg_size(), 8192);
        assert!(q.is_empty());
        assert!(!q.is_full());
    }

    #[test]
    fn test_queue_priority_order() {
        let mut q = MqQueue::new(None, 0);
        q.send(vec![1], 1).unwrap();
        q.send(vec![2], 5).unwrap();
        q.send(vec![3], 3).unwrap();

        let mut buf = [0u8; 10];
        let (len, prio) = q.receive(&mut buf).unwrap();
        assert_eq!(prio, 5);
        assert_eq!(&buf[..len], &[2]);

        let (len, prio) = q.receive(&mut buf).unwrap();
        assert_eq!(prio, 3);
        assert_eq!(&buf[..len], &[3]);

        let (len, prio) = q.receive(&mut buf).unwrap();
        assert_eq!(prio, 1);
        assert_eq!(&buf[..len], &[1]);
    }

    #[test]
    fn test_queue_full() {
        let attr = MqAttr {
            mq_maxmsg: 2,
            mq_msgsize: 10,
            ..MqAttr::default_attrs()
        };
        let mut q = MqQueue::new(Some(attr), 0);
        q.send(vec![1], 1).unwrap();
        q.send(vec![2], 1).unwrap();
        assert!(q.is_full());
        assert!(q.send(vec![3], 1).is_err());
    }

    #[test]
    fn test_queue_nonblock() {
        let attr = MqAttr {
            mq_maxmsg: 1,
            mq_msgsize: 10,
            ..MqAttr::default_attrs()
        };
        let mut q = MqQueue::new(Some(attr), O_NONBLOCK);
        q.send(vec![1], 1).unwrap();
        assert!(q.send(vec![2], 1).is_err());
        let mut buf = [0u8; 10];
        q.receive(&mut buf).unwrap();
        assert!(q.receive(&mut buf).is_err());
    }
}
