//! Message queue core data structure.

use crate::sync::IrqSafeMutex as Mutex;
use crate::syscalls::errno::Errno;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

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

/// Message queue state, shared by every descriptor open on the name.
pub struct MqQueue {
    /// Messages sorted by priority (highest first).
    msgs: VecDeque<MqMsg>,
    /// Maximum number of messages in queue.
    max_msgs: usize,
    /// Maximum size of a single message.
    max_msg_size: usize,
}

impl MqQueue {
    /// Create a new message queue with the given attributes.
    pub fn new(attr: MqAttr) -> Self {
        MqQueue {
            msgs: VecDeque::new(),
            max_msgs: attr.mq_maxmsg as usize,
            max_msg_size: attr.mq_msgsize as usize,
        }
    }

    /// Get current queue attributes. `mq_flags` belong to the open file
    /// description, not the queue, so the caller supplies them.
    pub fn get_attr(&self, flags: i32) -> MqAttr {
        MqAttr {
            mq_flags: flags as i64,
            mq_maxmsg: self.max_msgs as i64,
            mq_msgsize: self.max_msg_size as i64,
            mq_curmsgs: self.msgs.len() as i64,
            __reserved: [0; 4],
        }
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

    /// Send a message to the queue.
    /// `EAGAIN` means the queue is full; the caller decides whether to block.
    pub fn send(&mut self, data: &[u8], prio: u32) -> Result<(), Errno> {
        if prio >= MQ_PRIO_MAX {
            return Err(Errno::EINVAL);
        }
        if data.len() > self.max_msg_size {
            return Err(Errno::EMSGSIZE);
        }
        if self.is_full() {
            return Err(Errno::EAGAIN);
        }
        let msg = MqMsg {
            data: data.to_vec(),
            prio,
        };
        // Insert in priority order (highest priority first).
        let pos = self
            .msgs
            .iter()
            .position(|m| m.prio < prio)
            .unwrap_or(self.msgs.len());
        self.msgs.insert(pos, msg);
        Ok(())
    }

    /// Receive a message from the queue.
    /// `EAGAIN` means the queue is empty; the caller decides whether to block.
    /// The message stays queued if `buf` cannot hold it.
    pub fn receive(&mut self, buf: &mut [u8]) -> Result<(usize, u32), Errno> {
        let queued_len = match self.msgs.front() {
            Some(m) => m.data.len(),
            None => return Err(Errno::EAGAIN),
        };
        if queued_len > buf.len() {
            return Err(Errno::EMSGSIZE);
        }
        let msg = self.msgs.pop_front().ok_or(Errno::EAGAIN)?;
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
    pub fn new(attr: MqAttr) -> Self {
        MqQueueHandle {
            inner: Mutex::new(MqQueue::new(attr)),
        }
    }

    pub fn get_attr(&self, flags: i32) -> MqAttr {
        self.inner.lock().get_attr(flags)
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

    pub fn send(&self, data: &[u8], prio: u32) -> Result<(), Errno> {
        self.inner.lock().send(data, prio)
    }

    pub fn receive(&self, buf: &mut [u8]) -> Result<(usize, u32), Errno> {
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
        let q = MqQueue::new(MqAttr::default_attrs());
        assert_eq!(q.max_msgs(), 10);
        assert_eq!(q.max_msg_size(), 8192);
        assert!(q.is_empty());
        assert!(!q.is_full());
    }

    #[test]
    fn test_queue_priority_order() {
        let mut q = MqQueue::new(MqAttr::default_attrs());
        q.send(&[1], 1).unwrap();
        q.send(&[2], 5).unwrap();
        q.send(&[3], 3).unwrap();

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

        assert_eq!(q.receive(&mut buf).unwrap_err(), Errno::EAGAIN);
    }

    #[test]
    fn test_queue_full() {
        let attr = MqAttr {
            mq_maxmsg: 2,
            mq_msgsize: 10,
            ..MqAttr::default_attrs()
        };
        let mut q = MqQueue::new(attr);
        q.send(&[1], 1).unwrap();
        q.send(&[2], 1).unwrap();
        assert!(q.is_full());
        assert_eq!(q.send(&[3], 1).unwrap_err(), Errno::EAGAIN);
    }

    #[test]
    fn test_queue_rejects_oversize_and_keeps_message() {
        let attr = MqAttr {
            mq_maxmsg: 1,
            mq_msgsize: 10,
            ..MqAttr::default_attrs()
        };
        let mut q = MqQueue::new(attr);
        assert_eq!(q.send(&[0u8; 11], 1).unwrap_err(), Errno::EMSGSIZE);
        q.send(&[1, 2, 3], 1).unwrap();
        let mut short = [0u8; 2];
        assert_eq!(q.receive(&mut short).unwrap_err(), Errno::EMSGSIZE);
        assert_eq!(q.len(), 1);
        let mut buf = [0u8; 10];
        assert_eq!(q.receive(&mut buf).unwrap(), (3, 1));
    }
}
