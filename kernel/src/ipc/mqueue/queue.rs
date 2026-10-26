//! Message queue core data structure.

use crate::sync::IrqSafeMutex as Mutex;
use crate::syscalls::errno::Errno;
use crate::syscalls::posix_timers::SIGEV_SIGNAL;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;

const MQ_PRIO_MAX: u32 = 32768;

/// Per-message kernel overhead Linux charges to RLIMIT_MSGQUEUE
/// (`struct msg_msg` plus one `posix_msg_tree_node`).
const MQ_MSG_OVERHEAD: u64 = 96;

/// A registered `mq_notify(2)` request. One-shot: removed when it fires.
#[derive(Clone, Copy)]
pub struct MqNotify {
    pub owner_pid: u64,
    pub signo: u32,
    pub kind: i32,
}

lazy_static::lazy_static! {
    /// Bytes charged per uid against RLIMIT_MSGQUEUE.
    static ref MQ_BYTES: Mutex<BTreeMap<u32, u64>> = Mutex::new(BTreeMap::new());
}

/// Worst-case bytes Linux charges a queue at creation time.
pub fn message_bytes(attr: &MqAttr) -> Result<u64, Errno> {
    let maxmsg = attr.mq_maxmsg as u64;
    let payload = maxmsg
        .checked_mul(attr.mq_msgsize as u64)
        .ok_or(Errno::EOVERFLOW)?;
    let overhead = maxmsg
        .checked_mul(MQ_MSG_OVERHEAD)
        .ok_or(Errno::EOVERFLOW)?;
    payload.checked_add(overhead).ok_or(Errno::EOVERFLOW)
}

/// Reserve `bytes` against `uid`'s RLIMIT_MSGQUEUE budget.
pub fn charge_msgbytes(uid: u32, bytes: u64, limit: u64) -> Result<(), Errno> {
    let mut map = MQ_BYTES.lock();
    let used = map.get(&uid).copied().unwrap_or(0);
    let total = used.checked_add(bytes).ok_or(Errno::EOVERFLOW)?;
    if total > limit {
        return Err(Errno::EMFILE);
    }
    map.insert(uid, total);
    Ok(())
}

fn release_msgbytes(uid: u32, bytes: u64) {
    let mut map = MQ_BYTES.lock();
    if let Some(used) = map.get_mut(&uid) {
        *used = used.saturating_sub(bytes);
        if *used == 0 {
            map.remove(&uid);
        }
    }
}

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
    /// One-shot `mq_notify(2)` registration.
    notify: Option<MqNotify>,
}

impl MqQueue {
    /// Create a new message queue with the given attributes.
    pub fn new(attr: MqAttr) -> Self {
        MqQueue {
            msgs: VecDeque::new(),
            max_msgs: attr.mq_maxmsg as usize,
            max_msg_size: attr.mq_msgsize as usize,
            notify: None,
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
    /// Returns the notification to deliver when this send filled an empty
    /// queue, matching `__do_notify` (only the empty → non-empty edge fires).
    pub fn send(&mut self, data: &[u8], prio: u32) -> Result<Option<MqNotify>, Errno> {
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
        Ok(if self.msgs.len() == 1 {
            self.notify.take()
        } else {
            None
        })
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

    /// Total bytes of queued message payloads (the `QSIZE` field).
    pub fn qsize(&self) -> usize {
        self.msgs.iter().map(|m| m.data.len()).sum()
    }

    /// Register a one-shot notification. Linux returns EBUSY while any
    /// registration is live, including the caller's own.
    pub fn register_notify(&mut self, notify: MqNotify) -> Result<(), Errno> {
        if self.notify.is_some() {
            return Err(Errno::EBUSY);
        }
        self.notify = Some(notify);
        Ok(())
    }

    /// Drop the registration if `pid` owns it; a NULL `mq_notify` by any
    /// other process is silently ignored, as on Linux.
    pub fn clear_notify(&mut self, pid: u64) {
        if self.notify.map_or(false, |n| n.owner_pid == pid) {
            self.notify = None;
        }
    }

    /// The `read(2)` line Linux reports for a queue descriptor.
    pub fn describe_line(&self) -> String {
        let (kind, signo, pid) = match self.notify {
            Some(n) => (
                n.kind,
                if n.kind == SIGEV_SIGNAL { n.signo } else { 0 },
                n.owner_pid,
            ),
            None => (0, 0, 0),
        };
        alloc::format!(
            "QSIZE:{:<10} NOTIFY:{:<5} SIGNO:{:<5} NOTIFY_PID:{:<6}\n",
            self.qsize(),
            kind,
            signo,
            pid
        )
    }
}

/// Thread-safe message queue wrapper.
pub struct MqQueueHandle {
    inner: Mutex<MqQueue>,
    /// Uid charged for this queue and the bytes charged, released on drop.
    owner_uid: u32,
    charge: u64,
}

impl MqQueueHandle {
    pub fn new(attr: MqAttr, owner_uid: u32, charge: u64) -> Self {
        MqQueueHandle {
            inner: Mutex::new(MqQueue::new(attr)),
            owner_uid,
            charge,
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

    pub fn send(&self, data: &[u8], prio: u32) -> Result<Option<MqNotify>, Errno> {
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

    pub fn register_notify(&self, notify: MqNotify) -> Result<(), Errno> {
        self.inner.lock().register_notify(notify)
    }

    pub fn clear_notify(&self, pid: u64) {
        self.inner.lock().clear_notify(pid)
    }

    pub fn describe_line(&self) -> String {
        self.inner.lock().describe_line()
    }
}

impl Drop for MqQueueHandle {
    fn drop(&mut self) {
        if self.charge > 0 {
            release_msgbytes(self.owner_uid, self.charge);
        }
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
        let _ = q.send(&[1], 1).unwrap();
        let _ = q.send(&[2], 5).unwrap();
        let _ = q.send(&[3], 3).unwrap();

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
        let _ = q.send(&[1], 1).unwrap();
        let _ = q.send(&[2], 1).unwrap();
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
        let _ = q.send(&[1, 2, 3], 1).unwrap();
        let mut short = [0u8; 2];
        assert_eq!(q.receive(&mut short).unwrap_err(), Errno::EMSGSIZE);
        assert_eq!(q.len(), 1);
        let mut buf = [0u8; 10];
        assert_eq!(q.receive(&mut buf).unwrap(), (3, 1));
    }
}
