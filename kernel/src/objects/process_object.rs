use alloc::sync::Arc;
use crate::objects::{KernelObject, ObjectHeader, ObjectTypeId, security::SecurityDescriptor};
use crate::task::process::Process;
use spin::Mutex;

pub struct ProcessObject {
    pub header: ObjectHeader,
    pub inner: Arc<Mutex<Arc<Process>>>,
}

impl ProcessObject {
    pub fn new(process: Arc<Process>) -> Arc<Self> {
        let pid = process.id;
        let sec = SecurityDescriptor::new(0, 0, 0o600);
        let header = ObjectHeader::new(ObjectTypeId(9), sec);
        *header.name.lock() = Some(alloc::format!("Process/{}", pid));
        Arc::new(ProcessObject {
            header,
            inner: Arc::new(Mutex::new(process)),
        })
    }
}

impl KernelObject for ProcessObject {
    fn header(&self) -> &ObjectHeader { &self.header }
    fn type_name(&self) -> &'static str { "Process" }
    fn query_name(&self) -> Option<alloc::string::String> { self.header.name.lock().clone() }
    fn ioctl(&self, request: u64, _argp: *mut u8) -> Result<u64, ()> {
        match request {
            1 => {
                let proc = self.inner.lock();
                Ok(proc.id as u64)
            }
            _ => Err(()),
        }
    }
    fn read(&self, _offset: &mut u64, _buf: &mut [u8]) -> Result<usize, ()> {
        Err(())
    }
}
