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
