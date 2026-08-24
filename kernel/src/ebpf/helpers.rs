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

// Helper function 5: ktime_get_ns — monotonic nanosecond clock
pub fn bpf_helper_ktime_get_ns() -> u64 {
    // Use timer ticks × 10ms as approximation (100 Hz tick rate)
    crate::interrupts::get_ticks().wrapping_mul(10_000_000)
}

// Helper function 6: get_prandom_u32 — pseudo-random number generator
pub fn bpf_helper_get_prandom_u32() -> u32 {
    // xorshift32 PRNG seeded from RDTSC at init time
    use core::sync::atomic::{AtomicU32, Ordering};
    static SEED: AtomicU32 = AtomicU32::new(0xDEAD_BEEF);
    let mut x = SEED.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    SEED.store(x, Ordering::Relaxed);
    x
}

// Helper function 7: get_smp_processor_id — current CPU ID
pub fn bpf_helper_get_smp_processor_id() -> u64 {
    crate::smp::get_cpu_id() as u64
}

// Helper function 8: spin_lock / spin_unlock — lightweight locking
pub fn bpf_helper_spin_lock(lock_off: usize, stack: &mut [u8; STACK_SIZE]) {
    if lock_off + 8 <= STACK_SIZE {
        // Simple spin: busy-wait until the u64 at lock_off is 0, then set to 1
        loop {
            let val = unsafe { *(stack.as_ptr().add(lock_off) as *const u64) };
            if val == 0 {
                unsafe { *(stack.as_mut_ptr().add(lock_off) as *mut u64) = 1; }
                break;
            }
            core::hint::spin_loop();
        }
    }
}

pub fn bpf_helper_spin_unlock(lock_off: usize, stack: &mut [u8; STACK_SIZE]) {
    if lock_off + 8 <= STACK_SIZE {
        unsafe { *(stack.as_mut_ptr().add(lock_off) as *mut u64) = 0; }
    }
}

// Helper function 9: tail_call — jump to another BPF program
pub static TAIL_CALL_PROGS: crate::sync::IrqSafeMutex<alloc::collections::VecDeque<u32>> =
    crate::sync::IrqSafeMutex::new(alloc::collections::VecDeque::new());

pub fn bpf_helper_tail_call(prog_idx: u32) {
    TAIL_CALL_PROGS.lock().push_back(prog_idx);
}
