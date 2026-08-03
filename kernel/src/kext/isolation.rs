use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::sync::IrqSafeMutex as Mutex;
use crate::objects::{KernelObject, ObjectHeader, ObjectTypeId};
use crate::objects::handle::HandleTable;
use crate::objects::security::SecurityDescriptor;

/// Wraps a driver in a KernelObject for handle-based isolation.
/// If the driver crashes, its handle is closed, and the kernel
/// can restart it without system-wide panic (QNX-style).
pub struct DriverObject {
    pub header: ObjectHeader,
    pub driver_nub: Arc<dyn super::nub::Nub>,
    pub handle_table: Mutex<HandleTable>,
    pub crashed: AtomicBool,
}

impl DriverObject {
    pub fn new(nub: Arc<dyn super::nub::Nub>, name: &str) -> Arc<Self> {
        let header = ObjectHeader::new(
            ObjectTypeId(20),
            SecurityDescriptor::new(0, 0, 0o755),
        );
        *header.name.lock() = Some(alloc::string::String::from(name));
        Arc::new(DriverObject {
            header,
            driver_nub: nub,
            handle_table: Mutex::new(HandleTable::new()),
            crashed: AtomicBool::new(false),
        })
    }

    pub fn is_crashed(&self) -> bool { self.crashed.load(Ordering::Relaxed) }

    pub fn mark_crashed(&self) {
        self.crashed.store(true, Ordering::Relaxed);
    }

    pub fn restart(&self) -> bool {
        if !self.crashed.load(Ordering::Relaxed) { return true; }
        self.driver_nub.start();
        self.crashed.store(false, Ordering::Relaxed);
        true
    }
}

impl KernelObject for DriverObject {
    fn header(&self) -> &ObjectHeader { &self.header }
    fn type_name(&self) -> &'static str { "KextDriver" }
    fn ioctl(&self, request: u64, _argp: *mut u8) -> Result<u64, ()> {
        match request {
            1 => Ok(self.crashed.load(Ordering::Relaxed) as u64),
            2 => { self.restart(); Ok(1) }
            _ => Err(()),
        }
    }
    fn on_close(&self) {
        self.driver_nub.stop();
    }
}
