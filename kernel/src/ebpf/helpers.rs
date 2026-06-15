// ── eBPF helper functions callable from BPF programs ───────────────

// Helper function 1: map_lookup_elem
// R1 = map_fd, R2 = key_ptr, R3 = value_ptr
pub fn bpf_helper_map_lookup_elem(map_fd: u64, key_ptr: *const u8, value_ptr: *mut u8) -> i64 {
    let maps = super::maps::get_map(map_fd as usize);
    match maps {
        Some(m) => {
            let key = unsafe { core::slice::from_raw_parts(key_ptr, m.key_size()) };
            match m.lookup(key) {
                Some(val) => {
                    let copy_len = val.len().min(m.value_size());
                    unsafe { core::ptr::copy_nonoverlapping(val.as_ptr(), value_ptr, copy_len); }
                    0
                }
                None => -1,
            }
        }
        None => -2,
    }
}

pub fn bpf_helper_get_current_pid() -> u64 {
    let proc_lock = crate::task::process::CURRENT_PROCESS.lock();
    if let Some(ref proc) = *proc_lock {
        proc.id
    } else {
        0
    }
}

pub fn bpf_helper_get_ticks() -> u64 {
    crate::interrupts::get_ticks()
}

pub fn bpf_helper_debug_print(msg_ptr: *const u8, len: u64) {
    let msg = unsafe {
        core::str::from_utf8(core::slice::from_raw_parts(msg_ptr, len as usize))
    };
    if let Ok(s) = msg {
        crate::println!("[eBPF] {}", s);
    }
}
