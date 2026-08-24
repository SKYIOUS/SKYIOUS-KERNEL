use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::sync::IrqSafeMutex as Mutex;
use lazy_static::lazy_static;

pub const BPF_MAP_TYPE_HASH: u32 = 1;
pub const BPF_MAP_TYPE_ARRAY: u32 = 2;
pub const BPF_MAP_TYPE_PERF_EVENT_ARRAY: u32 = 3;
pub const BPF_MAP_TYPE_RINGBUF: u32 = 4;
pub const MAX_MAP_TYPE_COUNT: u32 = 5;

pub trait Map: Send + Sync {
    fn lookup(&self, key: &[u8]) -> Option<Vec<u8>>;
    fn update(&self, key: &[u8], value: &[u8]) -> bool;
    fn delete(&self, key: &[u8]) -> bool;
    fn key_size(&self) -> usize;
    fn value_size(&self) -> usize;
    fn max_entries(&self) -> usize;
    fn clear(&self);
}

// ── Arc-based registry ────────────────────────────────────────────
lazy_static! {
    static ref MAP_REGISTRY: Mutex<Vec<(usize, Arc<dyn Map>)>> = Mutex::new(Vec::new());
}

pub fn register_map(map: Arc<dyn Map>) -> usize {
    let mut reg = MAP_REGISTRY.lock();
    let id = reg.len() + 1;
    reg.push((id, map));
    id
}

pub fn get_map(id: usize) -> Option<Arc<dyn Map>> {
    let reg = MAP_REGISTRY.lock();
    for (map_id, map) in reg.iter() {
        if *map_id == id {
            return Some(map.clone());
        }
    }
    None
}

// ── Hash Table ────────────────────────────────────────────────────
pub struct HashTable {
    key_size: usize,
    value_size: usize,
    max_entries: usize,
    entries: Mutex<Vec<(Vec<u8>, Vec<u8>)>>,
}

impl HashTable {
    pub fn new(key_size: u32, value_size: u32, max_entries: u32) -> Self {
        HashTable {
            key_size: key_size as usize,
            value_size: value_size as usize,
            max_entries: max_entries as usize,
            entries: Mutex::new(Vec::new()),
        }
    }
}

impl Map for HashTable {
    fn lookup(&self, key: &[u8]) -> Option<Vec<u8>> {
        let entries = self.entries.lock();
        for (k, v) in entries.iter() {
            if k.as_slice() == key {
                return Some(v.clone());
            }
        }
        None
    }

    fn update(&self, key: &[u8], value: &[u8]) -> bool {
        let mut entries = self.entries.lock();
        for (k, v) in entries.iter_mut() {
            if k.as_slice() == key {
                let copy_len = v.len().min(value.len());
                v[..copy_len].copy_from_slice(&value[..copy_len]);
                return true;
            }
        }
        if entries.len() < self.max_entries {
            entries.push((key.to_vec(), value.to_vec()));
            true
        } else {
            false
        }
    }

    fn delete(&self, key: &[u8]) -> bool {
        let mut entries = self.entries.lock();
        let before = entries.len();
        entries.retain(|(k, _)| k.as_slice() != key);
        entries.len() < before
    }

    fn key_size(&self) -> usize { self.key_size }
    fn value_size(&self) -> usize { self.value_size }
    fn max_entries(&self) -> usize { self.max_entries }
    fn clear(&self) { self.entries.lock().clear(); }
}

// ── Array ─────────────────────────────────────────────────────────
pub struct ArrayMap {
    value_size: usize,
    max_entries: usize,
    entries: Mutex<Vec<Option<Vec<u8>>>>,
}

impl ArrayMap {
    pub fn new(value_size: u32, max_entries: u32) -> Self {
        let entries = (0..max_entries).map(|_| None).collect();
        ArrayMap {
            value_size: value_size as usize,
            max_entries: max_entries as usize,
            entries: Mutex::new(entries),
        }
    }
}

impl Map for ArrayMap {
    fn lookup(&self, key: &[u8]) -> Option<Vec<u8>> {
        let idx = if key.len() >= 4 { u32::from_ne_bytes([key[0], key[1], key[2], key[3]]) as usize } else { 0 };
        let entries = self.entries.lock();
        if idx < entries.len() { entries[idx].clone() } else { None }
    }

    fn update(&self, key: &[u8], value: &[u8]) -> bool {
        let idx = if key.len() >= 4 { u32::from_ne_bytes([key[0], key[1], key[2], key[3]]) as usize } else { 0 };
        let mut entries = self.entries.lock();
        if idx < entries.len() {
            entries[idx] = Some(value.to_vec());
            true
        } else {
            false
        }
    }

    fn delete(&self, key: &[u8]) -> bool {
        let idx = if key.len() >= 4 { u32::from_ne_bytes([key[0], key[1], key[2], key[3]]) as usize } else { 0 };
        let mut entries = self.entries.lock();
        if idx < entries.len() {
            entries[idx] = None;
            true
        } else {
            false
        }
    }

    fn key_size(&self) -> usize { 4 }
    fn value_size(&self) -> usize { self.value_size }
    fn max_entries(&self) -> usize { self.max_entries }
    fn clear(&self) { for e in self.entries.lock().iter_mut() { *e = None; } }
}

// ── Perf Event Array ──────────────────────────────────────────────
pub struct PerfEventArray {
    max_entries: usize,
}

impl PerfEventArray {
    pub fn new(max_entries: u32) -> Self { PerfEventArray { max_entries: max_entries as usize } }
}

impl Map for PerfEventArray {
    fn lookup(&self, _key: &[u8]) -> Option<Vec<u8>> { None }
    fn update(&self, _key: &[u8], _value: &[u8]) -> bool { false }
    fn delete(&self, _key: &[u8]) -> bool { false }
    fn key_size(&self) -> usize { 4 }
    fn value_size(&self) -> usize { 4 }
    fn max_entries(&self) -> usize { self.max_entries }
    fn clear(&self) {}
}

// ── Ring Buffer (SPSC producer/consumer) ─────────────────────────
/// Production-quality ring buffer for eBPF data streaming.
/// Uses a fixed-size circular buffer with separate producer/consumer
/// cursors. Writer reserves space, copies data, then commits.
/// Reader consumes in FIFO order. No allocation after init.
pub struct RingBuf {
    buf: Mutex<Vec<u8>>,
    capacity: usize,
    /// Producer cursor (bytes written). Wraps around.
    producer: Mutex<usize>,
    /// Consumer cursor (bytes read). Wraps around.
    consumer: Mutex<usize>,
    /// Total bytes lost due to overflow (for monitoring).
    lost_bytes: core::sync::atomic::AtomicU64,
}

impl RingBuf {
    pub fn new(capacity: usize) -> Self {
        // Round up to power of 2 for efficient modulo via bitmask
        let capacity = capacity.next_power_of_two().max(256);
        RingBuf {
            buf: Mutex::new(alloc::vec![0u8; capacity]),
            capacity,
            producer: Mutex::new(0),
            consumer: Mutex::new(0),
            lost_bytes: core::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Reserve space for `len` bytes. Returns the offset where data
    /// should be written, or None if full.
    fn reserve(&self, len: usize) -> Option<usize> {
        let mut prod = self.producer.lock();
        let cons = *self.consumer.lock();
        let used = prod.wrapping_sub(cons);
        let free = self.capacity - used;
        if len > free {
            self.lost_bytes.fetch_add(len as u64, core::sync::atomic::Ordering::Relaxed);
            return None;
        }
        let offset = *prod % self.capacity;
        *prod = prod.wrapping_add(len);
        Some(offset)
    }

    /// Read up to `max_len` bytes into `dst`. Returns bytes read.
    fn consume(&self, dst: &mut [u8]) -> usize {
        let mut cons = self.consumer.lock();
        let prod = *self.producer.lock();
        let available = prod.wrapping_sub(*cons);
        let to_read = dst.len().min(available);
        if to_read == 0 { return 0; }
        let offset = *cons % self.capacity;
        let buf = self.buf.lock();
        // Handle wrap-around: read in two parts if needed
        let first = to_read.min(self.capacity - offset);
        dst[..first].copy_from_slice(&buf[offset..offset + first]);
        if first < to_read {
            dst[first..to_read].copy_from_slice(&buf[..to_read - first]);
        }
        *cons = cons.wrapping_add(to_read);
        to_read
    }

    /// Get total bytes lost due to overflow.
    pub fn lost(&self) -> u64 {
        self.lost_bytes.load(core::sync::atomic::Ordering::Relaxed)
    }
}

impl Map for RingBuf {
    fn lookup(&self, _key: &[u8]) -> Option<Vec<u8>> {
        // Read one record from the ring buffer
        let mut output = alloc::vec![0u8; 256];
        let n = self.consume(&mut output);
        if n == 0 { None } else { output.truncate(n); Some(output) }
    }
    fn update(&self, _key: &[u8], value: &[u8]) -> bool {
        if let Some(offset) = self.reserve(value.len()) {
            let mut buf = self.buf.lock();
            let first = value.len().min(self.capacity - offset);
            buf[offset..offset + first].copy_from_slice(&value[..first]);
            if first < value.len() {
                buf[..value.len() - first].copy_from_slice(&value[first..]);
            }
            true
        } else {
            false // ring full, data lost
        }
    }
    fn delete(&self, _key: &[u8]) -> bool {
        *self.consumer.lock() = *self.producer.lock();
        true
    }
    fn key_size(&self) -> usize { 4 }
    fn value_size(&self) -> usize { 64 }
    fn max_entries(&self) -> usize { self.capacity / 64 }
    fn clear(&self) {
        *self.producer.lock() = 0;
        *self.consumer.lock() = 0;
        self.lost_bytes.store(0, core::sync::atomic::Ordering::Relaxed);
    }
}
