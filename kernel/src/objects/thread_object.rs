use alloc::sync::Arc;
use crate::objects::{KernelObject, ObjectHeader, ObjectTypeId, security::SecurityDescriptor};
use crate::task::thread::Thread;
use crate::sync::IrqSafeMutex as Mutex;

pub struct ThreadObject {
    pub header: ObjectHeader,
    pub inner: Arc<Mutex<Thread>>,
}

impl ThreadObject {
    pub fn new(thread: Thread) -> Arc<Self> {
        let sec = SecurityDescriptor::new(0, 0, 0o600);
        let header = ObjectHeader::new(ObjectTypeId(10), sec);
        *header.name.lock() = Some(alloc::format!("Thread/{:?}", thread._id));
        Arc::new(ThreadObject {
            header,
            inner: Arc::new(Mutex::new(thread)),
        })
    }
}

impl KernelObject for ThreadObject {
    fn header(&self) -> &ObjectHeader { &self.header }
    fn type_name(&self) -> &'static str { "Thread" }
    fn query_name(&self) -> Option<alloc::string::String> { self.header.name.lock().clone() }
    fn ioctl(&self, request: u64, argp: *mut u8) -> Result<u64, ()> {
        match request {
            1 => {
                // SAFETY: caller guarantees argp points to a valid u8 priority value
                let prio = unsafe { *(argp as *const u8) };
                let mut thread = self.inner.lock();
                thread.priority = prio;
                Ok(0)
            }
            _ => Err(()),
        }
    }
    fn on_close(&self) {}
}
