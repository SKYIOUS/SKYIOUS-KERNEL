#![allow(dead_code)]
use super::DirEntry;

pub fn parse_entries<F: FnMut(&DirEntry, &str)>(data: &[u8], mut callback: F) {
    let entry_size = core::mem::size_of::<DirEntry>();
    let mut off = 0usize;
    while off + entry_size <= data.len() {
        let raw = &data[off..];
        if raw.len() < entry_size { break; }
        let entry = unsafe { &*(raw.as_ptr() as *const DirEntry) };
        if entry.inode == 0 || entry.name_len == 0 { break; }
        if entry.rec_len == 0 { break; }
        let name_start = off + entry_size;
        let name_end = name_start + entry.name_len as usize;
        if name_end > data.len() { break; }
        if let Ok(name) = core::str::from_utf8(&data[name_start..name_end]) {
            callback(entry, name);
        }
        let step = entry.rec_len as usize;
        if step == 0 { break; }
        off += step;
    }
}
