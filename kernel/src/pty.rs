// Re-export all PTY types and functions from the crate.
pub use vahi_task::pty::*;

use crate::objects::{KernelObject, ObjectHeader, ObjectTypeId};
use alloc::sync::Arc;
use vahi_objects::security::SecurityDescriptor;
use vahi_sync::IrqSafeMutex as Mutex;

pub struct PtyMasterObject {
    pub header: ObjectHeader,
    pub idx: usize,
    pub pair: Arc<Mutex<PtyPair>>,
}

impl PtyMasterObject {
    pub fn new(idx: usize, pair: Arc<Mutex<PtyPair>>) -> Arc<Self> {
        Arc::new(PtyMasterObject {
            header: ObjectHeader::new(ObjectTypeId(7), SecurityDescriptor::default()),
            idx,
            pair,
        })
    }
}

impl KernelObject for PtyMasterObject {
    fn header(&self) -> &ObjectHeader {
        &self.header
    }

    fn read(&self, _offset: &mut u64, buf: &mut [u8]) -> Result<usize, ()> {
        pty_read_master(&self.pair, buf)
    }

    fn write(&self, _offset: &mut u64, buf: &[u8]) -> Result<usize, ()> {
        pty_write_master(&self.pair, buf)
    }

    fn poll_readable(&self) -> bool {
        let p = self.pair.lock();
        !p.master.buf.is_empty()
    }

    fn poll_writable(&self) -> bool {
        let p = self.pair.lock();
        !p.master.peer_closed
    }
}

pub struct PtySlaveObject {
    pub header: ObjectHeader,
    pub idx: usize,
    pub pair: Arc<Mutex<PtyPair>>,
}

impl PtySlaveObject {
    pub fn new(idx: usize, pair: Arc<Mutex<PtyPair>>) -> Arc<Self> {
        Arc::new(PtySlaveObject {
            header: ObjectHeader::new(ObjectTypeId(8), SecurityDescriptor::default()),
            idx,
            pair,
        })
    }
}

impl KernelObject for PtySlaveObject {
    fn header(&self) -> &ObjectHeader {
        &self.header
    }

    fn read(&self, _offset: &mut u64, buf: &mut [u8]) -> Result<usize, ()> {
        let ldisc = PtyLineDiscipline::default();
        pty_read_slave(&self.pair, buf, &ldisc)
    }

    fn write(&self, _offset: &mut u64, buf: &[u8]) -> Result<usize, ()> {
        pty_write_slave(&self.pair, buf)
    }

    fn poll_readable(&self) -> bool {
        let p = self.pair.lock();
        !p.slave.buf.is_empty()
    }

    fn poll_writable(&self) -> bool {
        let p = self.pair.lock();
        !p.slave.peer_closed
    }
}
