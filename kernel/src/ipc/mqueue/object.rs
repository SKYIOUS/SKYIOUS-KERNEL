//! Kernel object integration for message queues.

use crate::ipc::mqueue::queue::{MqAttr, MqQueueHandle};
use crate::objects::TYPE_MQUEUE;
use crate::sync::IrqSafeMutex as Mutex;
use alloc::sync::Arc;
use vahi_objects::{security::SecurityDescriptor, KernelObject, ObjectHeader, StatLike};

/// Kernel object representing a POSIX message queue.
pub struct MqObject {
    header: ObjectHeader,
    name: Mutex<Option<alloc::string::String>>,
    queue: MqQueueHandle,
}

impl MqObject {
    /// Create a new message queue kernel object.
    pub fn new(name: alloc::string::String, attr: Option<MqAttr>, flags: i32) -> Arc<Self> {
        let sec = SecurityDescriptor::default();
        let header = ObjectHeader::new(TYPE_MQUEUE, sec);
        Arc::new(MqObject {
            header,
            name: Mutex::new(Some(name)),
            queue: MqQueueHandle::new(attr, flags),
        })
    }

    /// Get the underlying queue handle.
    pub fn queue(&self) -> &MqQueueHandle {
        &self.queue
    }

    /// Get the queue name.
    pub fn name(&self) -> Option<alloc::string::String> {
        self.name.lock().clone()
    }
}

impl KernelObject for MqObject {
    fn header(&self) -> &ObjectHeader {
        &self.header
    }

    fn type_name(&self) -> &'static str {
        "MqQueue"
    }

    fn query_name(&self) -> Option<alloc::string::String> {
        self.name.lock().clone()
    }

    fn set_name(&self, name: &str) {
        *self.name.lock() = Some(alloc::string::String::from(name));
    }

    fn stat(&self) -> Result<StatLike, ()> {
        let attr = self.queue.get_attr();
        Ok(StatLike {
            ino: 0,
            mode: 0o100600,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: attr.mq_curmsgs as u64 * attr.mq_msgsize as u64,
            blocks: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mq_object_creation() {
        let obj = MqObject::new("/test_mq".into(), None, 0);
        assert_eq!(obj.type_id(), TYPE_MQUEUE);
        assert_eq!(obj.query_name(), Some("/test_mq".into()));
        assert_eq!(obj.type_name(), "MqQueue");
    }

    #[test]
    fn test_mq_object_queue_ops() {
        let obj = MqObject::new("/test_mq".into(), None, 0);
        obj.queue().send(vec![1, 2, 3], 5).unwrap();
        let mut buf = [0u8; 10];
        let (len, prio) = obj.queue().receive(&mut buf).unwrap();
        assert_eq!(prio, 5);
        assert_eq!(len, 3);
        assert_eq!(&buf[..len], &[1, 2, 3]);
    }
}
