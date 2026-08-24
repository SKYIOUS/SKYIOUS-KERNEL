//! timerfd_create, timerfd_settime, timerfd_gettime — file-descriptor based timers.
//!
//! Creates a file descriptor that becomes readable when a timer fires.
//! Used by epoll-based servers (nginx, redis, systemd) and async runtimes.
//!
//! Read semantics: returns 8-byte little-endian count of timer expirations
//! since last read. Blocks if 0 unless TFD_NONBLOCK.
//!
//! The timer fires by checking `wake_tick` against the global tick counter
//! in `check_timerfds()`, which is called from the scheduler tick handler.

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::sync::IrqSafeMutex as Mutex;
use crate::task::process::{CURRENT_PROCESS, FileDescriptor, TimerFdData};

// ─── Constants ───────────────────────────────────────────────────

pub const CLOCK_REALTIME: u32 = 0;
pub const CLOCK_MONOTONIC: u32 = 1;
pub const CLOCK_BOOTTIME: u32 = 4;

pub const TFD_CLOEXEC: u64 = 0x80000;
pub const TFD_NONBLOCK: u64 = 0x800;
pub const TFD_TIMER_ABSTIME: u64 = 1;

/// Tick rate: 100 Hz → 10ms per tick → 10_000_000 ns per tick.
const NS_PER_TICK: u64 = 10_000_000;

/// Unique key generator for timerfd blocking/wake.
static NEXT_TFD_KEY: AtomicU64 = AtomicU64::new(0x5000_0000_0000);

fn next_key() -> u64 {
    NEXT_TFD_KEY.fetch_add(1, Ordering::Relaxed)
}

// ─── itimerspec ──────────────────────────────────────────────────

#[repr(C)]
struct ITimerspec {
    it_interval_sec: i64,
    it_interval_nsec: i64,
    it_value_sec: i64,
    it_value_nsec: i64,
}

// ─── timerfd_create ──────────────────────────────────────────────

/// `timerfd_create(clockid, flags) → fd`
///
/// Creates a timerfd file descriptor.
///
/// # Arguments
/// * `clockid` — CLOCK_REALTIME (0), CLOCK_MONOTONIC (1), or CLOCK_BOOTTIME (4)
/// * `flags` — TFD_NONBLOCK (0x800), TFD_CLOEXEC (0x80000)
pub fn sys_timerfd_create(clockid: u64, flags: u64) -> u64 {
    match clockid as u32 {
        CLOCK_REALTIME | CLOCK_MONOTONIC | CLOCK_BOOTTIME => {}
        _ => return crate::syscalls::errno::Errno::EINVAL as u64,
    }

    let nonblock = (flags & TFD_NONBLOCK) != 0;
    let cloexec = (flags & TFD_CLOEXEC) != 0;

    let tfd = Arc::new(Mutex::new(TimerFdData {
        clock_id: clockid as u32,
        nonblock,
        it_interval_ns: 0,
        it_value_ns: 0,
        expirations: 0,
        wake_tick: 0,
        armed: false,
        key: next_key(),
    }));

    let lock = match *CURRENT_PROCESS.lock() {
        Some(ref p) => Arc::clone(p),
        None => return crate::syscalls::errno::Errno::ESRCH as u64,
    };

    let mut files = lock.files.lock();
    let fd_num = {
        let mut found = None;
        for (i, slot) in files.fd_table.iter().enumerate() {
            if slot.is_none() {
                found = Some(i);
                break;
            }
        }
        match found {
            Some(i) => {
                files.fd_table[i] = Some(FileDescriptor::TimerFd(tfd));
                i
            }
            None => {
                files.fd_table.push(Some(FileDescriptor::TimerFd(tfd)));
                files.fd_table.len() - 1
            }
        }
    };

    if cloexec {
        if fd_num >= files.fd_flags.len() {
            files.fd_flags.resize(fd_num + 1, 0);
        }
        files.fd_flags[fd_num] |= TFD_CLOEXEC;
    }

    fd_num as u64
}

// ─── timerfd_settime ─────────────────────────────────────────────

/// `timerfd_settime(fd, flags, new_value, old_value) → 0`
///
/// Arms or disarms the timerfd.
///
/// * `new_value` — pointer to `itimerspec { it_interval, it_value }`
/// * `old_value` — if non-null, receives previous timer state
/// * `flags` — TFD_TIMER_ABSTIME (1) for absolute time
pub fn sys_timerfd_settime(fd: u64, flags: u64, new_value_ptr: *const u8, old_value_ptr: *mut u8) -> u64 {
    if new_value_ptr.is_null() {
        return crate::syscalls::errno::Errno::EINVAL as u64;
    }

    let mut new_val = ITimerspec {
        it_interval_sec: 0,
        it_interval_nsec: 0,
        it_value_sec: 0,
        it_value_nsec: 0,
    };
    unsafe {
        if crate::syscalls::user_access::copy_from_user(
            core::slice::from_raw_parts_mut(&mut new_val as *mut _ as *mut u8, core::mem::size_of::<ITimerspec>()),
            new_value_ptr,
        ).is_err() {
            return crate::syscalls::errno::Errno::EFAULT as u64;
        }
    }

    let abstime = (flags & TFD_TIMER_ABSTIME) != 0;
    let it_value_ns = (new_val.it_value_sec as u64) * 1_000_000_000
        .wrapping_add(new_val.it_value_nsec as u64);
    let it_interval_ns = (new_val.it_interval_sec as u64) * 1_000_000_000
        .wrapping_add(new_val.it_interval_nsec as u64);

    let proc = match *CURRENT_PROCESS.lock() {
        Some(ref p) => Arc::clone(p),
        None => return crate::syscalls::errno::Errno::ESRCH as u64,
    };

    let files = proc.files.lock();
    if fd as usize >= files.fd_table.len() {
        return crate::syscalls::errno::Errno::EBADF as u64;
    }

    let tfd_arc = match files.fd_table[fd as usize] {
        Some(FileDescriptor::TimerFd(ref arc)) => Arc::clone(arc),
        _ => return crate::syscalls::errno::Errno::EINVAL as u64,
    };
    drop(files);

    // Write old value if requested
    if !old_value_ptr.is_null() {
        let t = tfd_arc.lock();
        let mut old = ITimerspec {
            it_interval_sec: 0,
            it_interval_nsec: 0,
            it_value_sec: 0,
            it_value_nsec: 0,
        };
        if t.armed {
            // Compute remaining time from wake_tick
            let now_ticks = crate::interrupts::get_ticks();
            let remaining_ticks = t.wake_tick.saturating_sub(now_ticks);
            let remaining_ns = remaining_ticks * NS_PER_TICK;
            old.it_value_sec = (remaining_ns / 1_000_000_000) as i64;
            old.it_value_nsec = (remaining_ns % 1_000_000_000) as i64;
            old.it_interval_sec = (t.it_interval_ns / 1_000_000_000) as i64;
            old.it_interval_nsec = (t.it_interval_ns % 1_000_000_000) as i64;
        }
        drop(t);
        unsafe {
            let _ = crate::syscalls::user_access::copy_to_user(
                old_value_ptr,
                core::slice::from_raw_parts(&old as *const _ as *const u8, core::mem::size_of::<ITimerspec>()),
            );
        }
    }

    let mut t = tfd_arc.lock();
    if it_value_ns == 0 {
        // Disarm
        t.armed = false;
        t.it_value_ns = 0;
        t.it_interval_ns = 0;
        t.wake_tick = 0;
    } else {
        // Arm
        let now_ticks = crate::interrupts::get_ticks();
        if abstime {
            // Absolute time: convert the it_value to a tick and compute delta from now
            let abs_ns = it_value_ns;
            let now_ns = now_ticks * NS_PER_TICK;
            let delta_ns = if abs_ns > now_ns { abs_ns - now_ns } else { 0 };
            let ticks = (delta_ns + NS_PER_TICK - 1) / NS_PER_TICK;
            t.it_value_ns = it_value_ns;
            t.it_interval_ns = it_interval_ns;
            t.wake_tick = now_ticks + ticks;
        } else {
            // Relative time
            let ticks = (it_value_ns + NS_PER_TICK - 1) / NS_PER_TICK;
            t.it_value_ns = it_value_ns;
            t.it_interval_ns = it_interval_ns;
            t.wake_tick = now_ticks + ticks;
        }
        t.armed = true;
    }
    drop(t);

    // Wake any threads blocked on reading this timerfd (e.g. if it was re-armed)
    let key = tfd_arc.lock().key;
    drop(tfd_arc);
    crate::task::scheduler::wake_pipe(key);

    0
}

// ─── timerfd_gettime ─────────────────────────────────────────────

/// `timerfd_gettime(fd, cur_value) → 0`
pub fn sys_timerfd_gettime(fd: u64, cur_value_ptr: *mut u8) -> u64 {
    if cur_value_ptr.is_null() {
        return crate::syscalls::errno::Errno::EINVAL as u64;
    }

    let proc = match *CURRENT_PROCESS.lock() {
        Some(ref p) => Arc::clone(p),
        None => return crate::syscalls::errno::Errno::ESRCH as u64,
    };

    let files = proc.files.lock();
    if fd as usize >= files.fd_table.len() {
        return crate::syscalls::errno::Errno::EBADF as u64;
    }

    let tfd_arc = match files.fd_table[fd as usize] {
        Some(FileDescriptor::TimerFd(ref arc)) => Arc::clone(arc),
        _ => return crate::syscalls::errno::Errno::EINVAL as u64,
    };
    drop(files);

    let t = tfd_arc.lock();
    let mut cur = ITimerspec {
        it_interval_sec: 0,
        it_interval_nsec: 0,
        it_value_sec: 0,
        it_value_nsec: 0,
    };
    if t.armed {
        let now_ticks = crate::interrupts::get_ticks();
        let remaining_ticks = t.wake_tick.saturating_sub(now_ticks);
        let remaining_ns = remaining_ticks * NS_PER_TICK;
        cur.it_value_sec = (remaining_ns / 1_000_000_000) as i64;
        cur.it_value_nsec = (remaining_ns % 1_000_000_000) as i64;
        cur.it_interval_sec = (t.it_interval_ns / 1_000_000_000) as i64;
        cur.it_interval_nsec = (t.it_interval_ns % 1_000_000_000) as i64;
    }
    drop(t);
    drop(tfd_arc);

    unsafe {
        if crate::syscalls::user_access::copy_to_user(
            cur_value_ptr,
            core::slice::from_raw_parts(&cur as *const _ as *const u8, core::mem::size_of::<ITimerspec>()),
        ).is_err() {
            return crate::syscalls::errno::Errno::EFAULT as u64;
        }
    }
    0
}

// ─── Tick hook ───────────────────────────────────────────────────

/// Called from the scheduler tick handler (IRQ context). Checks all armed
/// timerfd instances across all processes and increments expirations for
/// any that have reached their wake_tick.
///
/// Uses try_lock to avoid deadlocks in IRQ context — expired timers
/// re-fire on the next tick if the lock is contended.
pub fn check_timerfds() {
    let table = match crate::task::process::PROCESS_TABLE.try_lock() {
        Some(t) => t,
        None => return,
    };

    let current_tick = crate::interrupts::get_ticks();

    for (_, proc) in table.iter() {
        let files = match proc.files.try_lock() {
            Some(f) => f,
            None => continue,
        };

        for slot in files.fd_table.iter() {
            if let Some(FileDescriptor::TimerFd(ref tfd_arc)) = slot {
                let mut t = match tfd_arc.try_lock() {
                    Some(t) => t,
                    None => continue,
                };

                if !t.armed || t.wake_tick == 0 {
                    continue;
                }

                if current_tick >= t.wake_tick {
                    // Timer expired — increment expiration counter
                    t.expirations = t.expirations.saturating_add(1);

                    if t.it_interval_ns > 0 {
                        // Re-arm for the next interval
                        let interval_ticks = (t.it_interval_ns + NS_PER_TICK - 1) / NS_PER_TICK;
                        t.wake_tick = current_tick + interval_ticks;
                    } else {
                        // One-shot: disarm
                        t.armed = false;
                        t.wake_tick = 0;
                    }

                    // Wake any threads blocked reading from this timerfd
                    let key = t.key;
                    drop(t);
                    crate::task::scheduler::wake_pipe(key);
                }
            }
        }
    }
}
