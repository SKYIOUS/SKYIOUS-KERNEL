// ── eBPF helper functions callable from BPF programs ───────────────
use super::vm::STACK_SIZE;

// Helper function 1: map_lookup_elem
// R1 = map_fd, R2 = key offset (into the VM stack), R3 = value offset (into
// the VM stack). Values come from the BPF program's own stack buffer, never
// from raw kernel addresses, so both offsets are range-checked.
pub fn bpf_helper_map_lookup_elem(
    stack: &mut [u8; STACK_SIZE],
    map_fd: u64,
    key_off: usize,
    val_off: usize,
) -> i64 {
    let maps = super::maps::get_map(map_fd as usize);
    match maps {
        Some(m) => {
            let ksz = m.key_size();
            let vsz = m.value_size();
            if key_off.checked_add(ksz).map_or(true, |e| e > STACK_SIZE) { return -1; }
            if val_off.checked_add(vsz).map_or(true, |e| e > STACK_SIZE) { return -1; }
            let mut key = [0u8; STACK_SIZE];
            key[..ksz].copy_from_slice(&stack[key_off..key_off + ksz]);
            match m.lookup(&key[..ksz]) {
                Some(val) => {
                    let copy_len = val.len().min(vsz);
                    stack[val_off..val_off + copy_len].copy_from_slice(&val[..copy_len]);
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

pub fn bpf_helper_debug_print(stack: &[u8; STACK_SIZE], msg_off: usize, len: usize) {
    if msg_off.checked_add(len).map_or(true, |e| e > STACK_SIZE) { return; }
    let s = match core::str::from_utf8(&stack[msg_off..msg_off + len]) {
        Ok(s) => s,
        Err(_) => return,
    };
    crate::println!("[eBPF] {}", s);
}
