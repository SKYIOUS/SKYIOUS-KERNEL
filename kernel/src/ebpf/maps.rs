use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
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

// ── Ring Buffer ───────────────────────────────────────────────────
pub struct RingBuf {
    buffer: Mutex<Vec<u8>>,
    capacity: usize,
}

impl RingBuf {
    pub fn new(capacity: usize) -> Self { RingBuf { buffer: Mutex::new(Vec::with_capacity(capacity)), capacity } }
}

impl Map for RingBuf {
    fn lookup(&self, _key: &[u8]) -> Option<Vec<u8>> { let b = self.buffer.lock(); if b.is_empty() { None } else { Some(b.clone()) } }
    fn update(&self, _key: &[u8], value: &[u8]) -> bool {
        let mut buf = self.buffer.lock();
        if buf.len() + value.len() <= self.capacity { buf.extend_from_slice(value); true } else { false }
    }
    fn delete(&self, _key: &[u8]) -> bool { self.buffer.lock().clear(); true }
    fn key_size(&self) -> usize { 4 }
    fn value_size(&self) -> usize { 4 }
    fn max_entries(&self) -> usize { self.capacity / 64 }
    fn clear(&self) { self.buffer.lock().clear(); }
}
