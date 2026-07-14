use alloc::vec::Vec;
use alloc::sync::Arc;
use spin::Mutex;
use lazy_static::lazy_static;
use crossbeam_queue::ArrayQueue;

const PTY_BUF_SIZE: usize = 4096;
const MAX_PTYS: usize = 16;

pub struct PtyPair {
    pub master: PtyEnd,
    pub slave: PtyEnd,
}

pub struct PtyEnd {
    pub buf: ArrayQueue<u8>,
    pub peer_closed: bool,
}

impl PtyEnd {
    pub fn new() -> Self {
        PtyEnd { buf: ArrayQueue::new(PTY_BUF_SIZE), peer_closed: false }
    }
}

pub struct PtyLineDiscipline {
    pub echo: bool,
    pub canonical: bool,
}

impl Default for PtyLineDiscipline {
    fn default() -> Self {
        PtyLineDiscipline { echo: true, canonical: true }
    }
}

lazy_static! {
    pub static ref PTY_PAIRS: Mutex<Vec<Option<Arc<Mutex<PtyPair>>>>> = Mutex::new({
        let mut v = Vec::new();
        for _ in 0..MAX_PTYS { v.push(None); }
        v
    });
}

pub fn alloc_pty() -> Option<(usize, Arc<Mutex<PtyPair>>)> {
    let mut pairs = PTY_PAIRS.lock();
    for (idx, slot) in pairs.iter_mut().enumerate() {
        if slot.is_none() {
            let pair = Arc::new(Mutex::new(PtyPair {
                master: PtyEnd::new(),
                slave: PtyEnd::new(),
            }));
            *slot = Some(pair.clone());
            return Some((idx, pair));
        }
    }
    None
}

pub fn free_pty(idx: usize) {
    let mut pairs = PTY_PAIRS.lock();
    if idx < pairs.len() {
        pairs[idx] = None;
    }
}

pub fn pty_write_master(pair: &Arc<Mutex<PtyPair>>, data: &[u8]) -> Result<usize, ()> {
    let p = pair.lock();
    let mut written = 0;
    for &b in data {
        if p.slave.buf.push(b).is_err() { break; }
        written += 1;
    }
    Ok(written)
}

pub fn pty_write_slave(pair: &Arc<Mutex<PtyPair>>, data: &[u8]) -> Result<usize, ()> {
    let p = pair.lock();
    let mut written = 0;
    for &b in data {
        if p.master.buf.push(b).is_err() { break; }
        written += 1;
    }
    Ok(written)
}

pub fn pty_read_master(pair: &Arc<Mutex<PtyPair>>, buf: &mut [u8]) -> Result<usize, ()> {
    let p = pair.lock();
    let mut count = 0;
    while count < buf.len() {
        match p.master.buf.pop() {
            Some(b) => { buf[count] = b; count += 1; }
            None => break,
        }
    }
    if count == 0 && p.master.peer_closed { return Err(()); }
    Ok(count)
}

pub fn pty_read_slave(pair: &Arc<Mutex<PtyPair>>, buf: &mut [u8], ldisc: &PtyLineDiscipline) -> Result<usize, ()> {
    let p = pair.lock();
    if ldisc.canonical {
        let mut count = 0;
        while count < buf.len() {
            match p.slave.buf.pop() {
                Some(b'\n') | Some(b'\r') => {
                    buf[count] = b'\n';
                    count += 1;
                    return Ok(count);
                }
                Some(b) => { buf[count] = b; count += 1; }
                None => break,
            }
        }
        if count == 0 && p.slave.peer_closed { return Err(()); }
        Ok(count)
    } else {
        let mut count = 0;
        while count < buf.len() {
            match p.slave.buf.pop() {
                Some(b) => { buf[count] = b; count += 1; }
                None => break,
            }
        }
        if count == 0 && p.slave.peer_closed { return Err(()); }
        Ok(count)
    }
}

// ─── PtyMasterObject / PtySlaveObject: wrap PtyPair as KernelObject ──

use crate::objects::{KernelObject, ObjectHeader, ObjectTypeId, security::SecurityDescriptor};

#[allow(dead_code)]
pub struct PtyMasterObject {
    pub header: ObjectHeader,
    pub idx: usize,
    pub pair: Arc<Mutex<PtyPair>>,
}

#[allow(dead_code)]
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
    fn header(&self) -> &ObjectHeader { &self.header }

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

#[allow(dead_code)]
pub struct PtySlaveObject {
    pub header: ObjectHeader,
    pub idx: usize,
    pub pair: Arc<Mutex<PtyPair>>,
}

#[allow(dead_code)]
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
    fn header(&self) -> &ObjectHeader { &self.header }

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
