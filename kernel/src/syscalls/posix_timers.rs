use crate::sync::IrqSafeMutex as Mutex;
use hashbrown::HashMap;
use core::sync::atomic::{AtomicI32, Ordering};
use lazy_static::lazy_static;

pub struct PosixTimer {
    pub tid: i32,
    pub sigev_signo: i32,
    pub sigev_value: i64,
    pub sigev_notify: i32,
    pub clock_id: i32,
    pub interval: u64,
    pub value: u64,
    pub overrun: i32,
    pub owner_pid: u64,
    pub active: bool,
}

lazy_static! {
    pub static ref POSIX_TIMERS: Mutex<HashMap<i32, PosixTimer>> = Mutex::new(HashMap::new());
}
pub static NEXT_TIMER_ID: AtomicI32 = AtomicI32::new(1);

pub const SIGEV_SIGNAL: i32 = 0;
pub const SIGEV_NONE: i32 = 1;
pub const SIGEV_THREAD: i32 = 2;

pub const CLOCK_REALTIME: i32 = 0;
pub const CLOCK_MONOTONIC: i32 = 1;

pub const TIMER_ABSTIME: i32 = 1;

#[repr(C)]
pub struct sigevent {
    pub sigev_value: i64,
    pub sigev_signo: i32,
    pub sigev_notify: i32,
}

#[repr(C)]
pub struct timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

#[repr(C)]
pub struct itimerspec {
    pub it_interval: timespec,
    pub it_value: timespec,
}

fn timespec_to_ns(ts: &timespec) -> u64 {
    if ts.tv_sec < 0 || ts.tv_nsec < 0 {
        return 0;
    }
    let sec_ns = (ts.tv_sec as u64).saturating_mul(1_000_000_000);
    sec_ns.saturating_add(ts.tv_nsec as u64)
}

fn ns_to_timespec(ns: u64) -> timespec {
    timespec {
        tv_sec: (ns / 1_000_000_000) as i64,
        tv_nsec: (ns % 1_000_000_000) as i64,
    }
}

pub fn get_current_time_ns() -> u64 {
    crate::interrupts::get_ticks() * 10_000_000
}

pub fn check_posix_timers() {
    // Called from IRQ context (timer tick): the IRQ may have preempted the
    // holder of these locks, and a spin here would deadlock the CPU. Skip the
    // pass on contention — expired timers re-expire on the next tick.
    if POSIX_TIMERS.try_lock().is_none()
        || crate::task::process::PROCESS_TABLE.try_lock().is_none()
    {
        return;
    }
    let now = get_current_time_ns();
    let mut timers = POSIX_TIMERS.lock();

    // One-pass in place: no Vec collect, no allocation in IRQ context.
    // ponytail: mutating during iter_mut is safe — no structural changes here.
    for (_, expired) in timers.iter_mut() {
        if !expired.active || expired.value == 0 || now < expired.value {
            continue;
        }

        if expired.overrun < i32::MAX {
            expired.overrun += 1;
        }

        if expired.sigev_notify == SIGEV_SIGNAL {
            let table = crate::task::process::PROCESS_TABLE.lock();
            if let Some(proc) = table.get(&expired.owner_pid) {
                if let Some(mut sigstate) = proc.signals.try_lock() {
                    let bit = (expired.sigev_signo as u32).wrapping_sub(1);
                    if bit < 64 {
                        sigstate.pending |= 1u64 << bit;
                    }
                }
            }
        }

        if expired.interval > 0 {
            expired.value = now + expired.interval;
            expired.overrun = 0;
        } else {
            expired.active = false;
        }
    }
}

// ─── timer_create ─────────────────────────────────────────────────

pub fn sys_timer_create(clockid: i32, sevp: *const sigevent, timerid: *mut i32) -> u64 {
    if clockid != CLOCK_REALTIME && clockid != CLOCK_MONOTONIC {
        return crate::syscalls::errno::Errno::EINVAL as u64;
    }

    let (notify, signo, value) = if sevp.is_null() {
        (SIGEV_SIGNAL, 14i32, 0i64)
    } else {
        let mut ev = sigevent { sigev_value: 0, sigev_signo: 14, sigev_notify: 0 };
        let slice = unsafe {
            core::slice::from_raw_parts_mut(&mut ev as *mut sigevent as *mut u8, core::mem::size_of::<sigevent>())
        };
        if unsafe { crate::syscalls::user_access::copy_from_user(slice, sevp as *const u8) }.is_err() {
            return crate::syscalls::errno::Errno::EFAULT as u64;
        }
        if ev.sigev_notify == SIGEV_THREAD {
            return crate::syscalls::errno::Errno::ENOSYS as u64;
        }
        if ev.sigev_notify != SIGEV_SIGNAL && ev.sigev_notify != SIGEV_NONE {
            return crate::syscalls::errno::Errno::EINVAL as u64;
        }
        if ev.sigev_signo < 1 || ev.sigev_signo > 32 {
            return crate::syscalls::errno::Errno::EINVAL as u64;
        }
        (ev.sigev_notify, ev.sigev_signo, ev.sigev_value)
    };

    let pid = {
        let lock = crate::task::process::CURRENT_PROCESS.lock();
        lock.as_ref().map(|p| p.id).unwrap_or(0)
    };
    if pid == 0 {
        return crate::syscalls::errno::Errno::ESRCH as u64;
    }

    let tid = NEXT_TIMER_ID.fetch_add(1, Ordering::Relaxed);

    let timer = PosixTimer {
        tid,
        sigev_signo: signo,
        sigev_value: value,
        sigev_notify: notify,
        clock_id: clockid,
        interval: 0,
        value: 0,
        overrun: 0,
        owner_pid: pid,
        active: false,
    };

    POSIX_TIMERS.lock().insert(tid, timer);

    if timerid.is_null() {
        POSIX_TIMERS.lock().remove(&tid);
        return crate::syscalls::errno::Errno::EFAULT as u64;
    }
    if unsafe { crate::syscalls::user_access::copy_to_user(timerid as *mut u8, &tid.to_ne_bytes()) }.is_err() {
        POSIX_TIMERS.lock().remove(&tid);
        return crate::syscalls::errno::Errno::EFAULT as u64;
    }

    0
}

// ─── timer_settime ────────────────────────────────────────────────

pub fn sys_timer_settime(timerid: i32, flags: i32, new_value: *const itimerspec, old_value: *mut itimerspec) -> u64 {
    let mut timers = POSIX_TIMERS.lock();
    let timer = match timers.get_mut(&timerid) {
        Some(t) => t,
        None => return crate::syscalls::errno::Errno::EINVAL as u64,
    };

    if !old_value.is_null() {
        let old_its = itimerspec {
            it_interval: ns_to_timespec(timer.interval),
            it_value: if timer.active && timer.value > 0 {
                let now = get_current_time_ns();
                if now >= timer.value {
                    timespec { tv_sec: 0, tv_nsec: 0 }
                } else {
                    ns_to_timespec(timer.value - now)
                }
            } else {
                timespec { tv_sec: 0, tv_nsec: 0 }
            },
        };
        let slice = unsafe {
            core::slice::from_raw_parts(&old_its as *const itimerspec as *const u8, core::mem::size_of::<itimerspec>())
        };
        if unsafe { crate::syscalls::user_access::copy_to_user(old_value as *mut u8, slice) }.is_err() {
            return crate::syscalls::errno::Errno::EFAULT as u64;
        }
    }

    if !new_value.is_null() {
        let mut new_its = itimerspec {
            it_interval: timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: timespec { tv_sec: 0, tv_nsec: 0 },
        };
        let slice = unsafe {
            core::slice::from_raw_parts_mut(&mut new_its as *mut itimerspec as *mut u8, core::mem::size_of::<itimerspec>())
        };
        if unsafe { crate::syscalls::user_access::copy_from_user(slice, new_value as *const u8) }.is_err() {
            return crate::syscalls::errno::Errno::EFAULT as u64;
        }

        let interval = timespec_to_ns(&new_its.it_interval);
        let value_ns = timespec_to_ns(&new_its.it_value);

        if value_ns == 0 && (new_its.it_value.tv_sec != 0 || new_its.it_value.tv_nsec != 0) {
            timer.active = false;
            timer.value = 0;
        } else if value_ns == 0 {
            timer.active = false;
            timer.value = 0;
        } else {
            let now = get_current_time_ns();
            timer.value = if (flags & TIMER_ABSTIME) != 0 { value_ns } else { now + value_ns };
            timer.interval = interval;
            timer.overrun = 0;
            timer.active = true;
        }
    }

    0
}

// ─── timer_gettime ────────────────────────────────────────────────

pub fn sys_timer_gettime(timerid: i32, curr_value: *mut itimerspec) -> u64 {
    let timers = POSIX_TIMERS.lock();
    let timer = match timers.get(&timerid) {
        Some(t) => t,
        None => return crate::syscalls::errno::Errno::EINVAL as u64,
    };

    let rem = if timer.active && timer.value > 0 {
        let now = get_current_time_ns();
        if now >= timer.value {
            timespec { tv_sec: 0, tv_nsec: 0 }
        } else {
            ns_to_timespec(timer.value - now)
        }
    } else {
        timespec { tv_sec: 0, tv_nsec: 0 }
    };

    let its = itimerspec {
        it_interval: ns_to_timespec(timer.interval),
        it_value: rem,
    };

    let slice = unsafe {
        core::slice::from_raw_parts(&its as *const itimerspec as *const u8, core::mem::size_of::<itimerspec>())
    };
    if unsafe { crate::syscalls::user_access::copy_to_user(curr_value as *mut u8, slice) }.is_err() {
        return crate::syscalls::errno::Errno::EFAULT as u64;
    }

    0
}

// ─── timer_getoverrun ─────────────────────────────────────────────

pub fn sys_timer_getoverrun(timerid: i32) -> u64 {
    let mut timers = POSIX_TIMERS.lock();
    let timer = match timers.get_mut(&timerid) {
        Some(t) => t,
        None => return crate::syscalls::errno::Errno::EINVAL as u64,
    };
    let overrun = timer.overrun;
    timer.overrun = 0;
    overrun as u64
}

// ─── timer_delete ─────────────────────────────────────────────────

pub fn sys_timer_delete(timerid: i32) -> u64 {
    let mut timers = POSIX_TIMERS.lock();
    if timers.remove(&timerid).is_some() {
        0
    } else {
        crate::syscalls::errno::Errno::EINVAL as u64
    }
}

// ─── timerfd syscalls ─────────────────────────────────────────────
//
// timerfd_create, timerfd_settime, timerfd_gettime — create a file
// descriptor that becomes readable when a timer fires. Essential for
// epoll-based servers (nginx, redis, systemd).

use crate::task::process::{CURRENT_PROCESS, FileDescriptor, TimerFdData};
use alloc::sync::Arc;

const TFD_CLOEXEC: u32 = 0x80000;
const TFD_NONBLOCK: u32 = 0x800;
const TFD_TIMER_ABSTIME: u32 = 1;

/// timerfd_create(clockid, flags) → fd
///
/// Creates a timerfd. clockid: 0=CLOCK_REALTIME, 1=CLOCK_MONOTONIC.
pub fn sys_timerfd_create(clockid: u64, flags: u64) -> u64 {
    if clockid > 1 {
        return crate::syscalls::errno::Errno::EINVAL as u64;
    }
    let nonblock = (flags & TFD_NONBLOCK as u64) != 0;
    let _cloexec = (flags & TFD_CLOEXEC as u64) != 0;

    let tfd = Arc::new(Mutex::new(TimerFdData {
        clock_id: clockid as u32,
        nonblock,
        it_interval_ns: 0,
        it_value_ns: 0,
        expirations: 0,
        wake_tick: 0,
        armed: false,
        key: 0, // unused in legacy path
    }));

    let lock = CURRENT_PROCESS.lock();
    if let Some(ref proc) = *lock {
        let mut files = proc.files.lock();
        let fd_num = find_free_fd(&files.fd_table);
        if fd_num >= files.fd_table.len() {
            files.fd_table.resize(fd_num + 1, None);
        }
        files.fd_table[fd_num] = Some(FileDescriptor::TimerFd(tfd));
        fd_num as u64
    } else {
        crate::syscalls::errno::Errno::ESRCH as u64
    }
}

/// timerfd_settime(fd, flags, new_value, old_value) → 0
///
/// Arms or disarms the timerfd.
/// new_value: pointer to itimerspec { it_interval (sec,nsec), it_value (sec,nsec) }.
#[repr(C)]
struct ITimerspec {
    it_interval_sec: i64,
    it_interval_nsec: i64,
    it_value_sec: i64,
    it_value_nsec: i64,
}

pub fn sys_timerfd_settime(fd: u64, flags: u64, new_value_ptr: *const u8, old_value_ptr: *mut u8) -> u64 {
    if new_value_ptr.is_null() {
        return crate::syscalls::errno::Errno::EINVAL as u64;
    }

    let mut new_val = ITimerspec { it_interval_sec: 0, it_interval_nsec: 0, it_value_sec: 0, it_value_nsec: 0 };
    unsafe {
        if crate::syscalls::user_access::copy_from_user(
            core::slice::from_raw_parts_mut(&mut new_val as *mut _ as *mut u8, core::mem::size_of::<ITimerspec>()),
            new_value_ptr,
        ).is_err() {
            return crate::syscalls::errno::Errno::EFAULT as u64;
        }
    }

    let abstime = (flags & TFD_TIMER_ABSTIME as u64) != 0;
    let _ = abstime; // TODO: absolute time support

    let it_value_ns = (new_val.it_value_sec as u64) * 1_000_000_000 + (new_val.it_value_nsec as u64);
    let it_interval_ns = (new_val.it_interval_sec as u64) * 1_000_000_000 + (new_val.it_interval_nsec as u64);

    let lock = CURRENT_PROCESS.lock();
    if let Some(ref proc) = *lock {
        let files = proc.files.lock();
        if fd as usize >= files.fd_table.len() {
            return crate::syscalls::errno::Errno::EBADF as u64;
        }
        if let Some(FileDescriptor::TimerFd(ref tfd)) = files.fd_table[fd as usize] {
            // Write old value if requested
            if !old_value_ptr.is_null() {
                let mut old = ITimerspec {
                    it_interval_sec: 0, it_interval_nsec: 0,
                    it_value_sec: 0, it_value_nsec: 0,
                };
                {
                    let t = tfd.lock();
                    if t.armed {
                        old.it_value_sec = (t.it_value_ns / 1_000_000_000) as i64;
                        old.it_value_nsec = (t.it_value_ns % 1_000_000_000) as i64;
                        old.it_interval_sec = (t.it_interval_ns / 1_000_000_000) as i64;
                        old.it_interval_nsec = (t.it_interval_ns % 1_000_000_000) as i64;
                    }
                }
                unsafe {
                    let _ = crate::syscalls::user_access::copy_to_user(
                        old_value_ptr,
                        core::slice::from_raw_parts(&old as *const _ as *const u8, core::mem::size_of::<ITimerspec>()),
                    );
                }
            }

            let mut t = tfd.lock();
            if it_value_ns == 0 {
                // Disarm
                t.armed = false;
                t.it_value_ns = 0;
                t.it_interval_ns = 0;
                t.wake_tick = 0;
            } else {
                // Arm
                let ticks_per_ns: u64 = 100_000_000; // 100 Hz → 10ms per tick
                let now_ticks = crate::interrupts::get_ticks();
                let ticks = (it_value_ns + ticks_per_ns - 1) / ticks_per_ns;
                t.it_value_ns = it_value_ns;
                t.it_interval_ns = it_interval_ns;
                t.wake_tick = now_ticks + ticks;
                t.armed = true;
            }
            drop(t);
            drop(files);
            drop(lock);
            0
        } else {
            crate::syscalls::errno::Errno::EINVAL as u64
        }
    } else {
        crate::syscalls::errno::Errno::ESRCH as u64
    }
}

/// timerfd_gettime(fd, cur_value) → 0
pub fn sys_timerfd_gettime(fd: u64, cur_value_ptr: *mut u8) -> u64 {
    if cur_value_ptr.is_null() {
        return crate::syscalls::errno::Errno::EINVAL as u64;
    }

    let lock = CURRENT_PROCESS.lock();
    if let Some(ref proc) = *lock {
        let files = proc.files.lock();
        if fd as usize >= files.fd_table.len() {
            return crate::syscalls::errno::Errno::EBADF as u64;
        }
        if let Some(FileDescriptor::TimerFd(ref tfd)) = files.fd_table[fd as usize] {
            let t = tfd.lock();
            let mut cur = ITimerspec {
                it_interval_sec: 0, it_interval_nsec: 0,
                it_value_sec: 0, it_value_nsec: 0,
            };
            if t.armed {
                let now_ticks = crate::interrupts::get_ticks();
                let ticks_per_ns: u64 = 100_000_000;
                let remaining_ticks = t.wake_tick.saturating_sub(now_ticks);
                let remaining_ns = remaining_ticks * ticks_per_ns;
                cur.it_value_sec = (remaining_ns / 1_000_000_000) as i64;
                cur.it_value_nsec = (remaining_ns % 1_000_000_000) as i64;
                cur.it_interval_sec = (t.it_interval_ns / 1_000_000_000) as i64;
                cur.it_interval_nsec = (t.it_interval_ns % 1_000_000_000) as i64;
            }
            drop(t);
            drop(files);
            drop(lock);
            unsafe {
                if crate::syscalls::user_access::copy_to_user(
                    cur_value_ptr,
                    core::slice::from_raw_parts(&cur as *const _ as *const u8, core::mem::size_of::<ITimerspec>()),
                ).is_err() {
                    return crate::syscalls::errno::Errno::EFAULT as u64;
                }
            }
            0
        } else {
            crate::syscalls::errno::Errno::EINVAL as u64
        }
    } else {
        crate::syscalls::errno::Errno::ESRCH as u64
    }
}

fn find_free_fd(fd_table: &[Option<FileDescriptor>]) -> usize {
    for (i, slot) in fd_table.iter().enumerate() {
        if slot.is_none() {
            return i;
        }
    }
    fd_table.len()
}
