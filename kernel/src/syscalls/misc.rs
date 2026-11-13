//! misc syscalls — split from mod.rs (7246 lines).
use super::errno;
use super::*;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};

use crate::syscalls::user_access;
use core::sync::atomic::Ordering;

const POLLIN: i16 = 0x001;
const POLLOUT: i16 = 0x004;
const POLLNVAL: i16 = 0x020;

/// Compile-time clock resolution lookup.
/// Returns the resolution as (seconds, nanoseconds) for the given clock_id.
/// Returns (0, 0) for invalid/unsupported clocks.
const fn clock_resolution(clock_id: u64) -> (i64, i64) {
    const CLOCK_REALTIME: u64 = 0;
    const CLOCK_MONOTONIC: u64 = 1;
    const CLOCK_PROCESS_CPUTIME_ID: u64 = 2;
    const CLOCK_THREAD_CPUTIME_ID: u64 = 3;
    const CLOCK_MONOTONIC_RAW: u64 = 4;
    const CLOCK_REALTIME_COARSE: u64 = 5;
    const CLOCK_MONOTONIC_COARSE: u64 = 6;
    const CLOCK_BOOTTIME: u64 = 7;
    const CLOCK_REALTIME_ALARM: u64 = 8;
    const CLOCK_BOOTTIME_ALARM: u64 = 9;
    // SKYIOUS-native clocks
    const CLOCK_SKYIOUS_THREAD_CPUTIME: u64 = 100;
    const CLOCK_SKYIOUS_SCHED_DEADLINE: u64 = 101;
    const CLOCK_SKYIOUS_BOOT_NS: u64 = 102;
    const CLOCK_SKYIOUS_KERNEL_FLAGS: u64 = 103;
    const CLOCK_SKYIOUS_PROCESS_INFO: u64 = 104;

    match clock_id {
        CLOCK_REALTIME | CLOCK_MONOTONIC | CLOCK_MONOTONIC_RAW | CLOCK_BOOTTIME => (0, 1),
        CLOCK_REALTIME_COARSE | CLOCK_MONOTONIC_COARSE => (0, 1_000_000),
        CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => (0, 1),
        CLOCK_SKYIOUS_THREAD_CPUTIME | CLOCK_SKYIOUS_SCHED_DEADLINE | CLOCK_SKYIOUS_BOOT_NS
        | CLOCK_SKYIOUS_KERNEL_FLAGS | CLOCK_SKYIOUS_PROCESS_INFO => (0, 1),
        CLOCK_REALTIME_ALARM | CLOCK_BOOTTIME_ALARM => (0, 0),
        _ => (0, 0),
    }
}

/// Kernel-internal version of clock_gettime that writes directly to a kernel pointer.
/// Used by selftests and kernel code that needs time without copy_to_user overhead.
pub fn sys_clock_gettime_kernel(clock_id: u64, tp: &mut Timespec) -> u64 {
    const CLOCK_REALTIME: u64 = 0;
    const CLOCK_MONOTONIC: u64 = 1;
    const CLOCK_PROCESS_CPUTIME_ID: u64 = 2;
    const CLOCK_THREAD_CPUTIME_ID: u64 = 3;
    const CLOCK_MONOTONIC_RAW: u64 = 4;
    const CLOCK_REALTIME_COARSE: u64 = 5;
    const CLOCK_MONOTONIC_COARSE: u64 = 6;
    const CLOCK_BOOTTIME: u64 = 7;
    const CLOCK_REALTIME_ALARM: u64 = 8;
    const CLOCK_BOOTTIME_ALARM: u64 = 9;
    // SKYIOUS-native clocks
    const CLOCK_SKYIOUS_THREAD_CPUTIME: u64 = 100;
    const CLOCK_SKYIOUS_SCHED_DEADLINE: u64 = 101;
    const CLOCK_SKYIOUS_BOOT_NS: u64 = 102;
    const CLOCK_SKYIOUS_KERNEL_FLAGS: u64 = 103;
    const CLOCK_SKYIOUS_PROCESS_INFO: u64 = 104;

    // Fast path: try vDSO TSC-based time for CLOCK_MONOTONIC and CLOCK_REALTIME (x86_64 only)
    #[cfg(target_arch = "x86_64")]
    if matches!(clock_id, CLOCK_MONOTONIC | CLOCK_REALTIME) {
        let (sec, nsec) = crate::vdso_read_tsc_time(clock_id);
        if sec != 0 || nsec != 0 {
            tp.tv_sec = sec as i64;
            tp.tv_nsec = nsec as i64;
            return 0;
        }
    }

    // Fallback: slow path
    *tp = match clock_id {
        CLOCK_REALTIME => {
            let (sec, nsec) = crate::drivers::rtc::read_realtime();
            Timespec {
                tv_sec: sec,
                tv_nsec: nsec,
            }
        }
        CLOCK_MONOTONIC => {
            let ticks = crate::interrupts::get_ticks();
            let total_ms = ticks * 10;
            Timespec {
                tv_sec: (total_ms / 1000) as i64,
                tv_nsec: ((total_ms % 1000) * 1_000_000) as i64,
            }
        }
        CLOCK_MONOTONIC_RAW => {
            // Same as monotonic but without NTP adjustments - use tick-based
            let ticks = crate::interrupts::get_ticks();
            let total_ms = ticks * 10;
            Timespec {
                tv_sec: (total_ms / 1000) as i64,
                tv_nsec: ((total_ms % 1000) * 1_000_000) as i64,
            }
        }
        CLOCK_BOOTTIME => {
            // Monotonic time including suspend - use tick-based
            let ticks = crate::interrupts::get_ticks();
            let total_ms = ticks * 10;
            Timespec {
                tv_sec: (total_ms / 1000) as i64,
                tv_nsec: ((total_ms % 1000) * 1_000_000) as i64,
            }
        }
        CLOCK_REALTIME_COARSE | CLOCK_MONOTONIC_COARSE => {
            // Low-resolution versions - round to 1ms
            let ticks = crate::interrupts::get_ticks();
            let total_ms = ticks * 10;
            let coarse_sec = (total_ms / 1000) as i64;
            Timespec {
                tv_sec: coarse_sec,
                tv_nsec: 0,
            }
        }
        CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            // Process/thread CPU time — read from scheduler + in-flight slice
            #[cfg(target_arch = "x86_64")]
            {
                use crate::task::scheduler::this_cpu_sched;
                use core::sync::atomic::Ordering;
                
                let tsc_now = {
                    let (lo, hi): (u32, u32);
                    unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi) };
                    ((hi as u64) << 32) | (lo as u64)
                };
                
                let ts = if let Some(sched) = this_cpu_sched().try_lock() {
                    if let Some(ref cur) = sched.current_thread {
                        if clock_id == CLOCK_THREAD_CPUTIME_ID {
                            // Thread CPU time: thread's accumulated + in-flight slice
                            let tsc_delta = tsc_now.wrapping_sub(cur.slice_start_tsc);
                            let ns = {
                                let vdso_ptr = crate::vdso_page();
                                if !vdso_ptr.is_null() {
                                    let vdso = unsafe { &*vdso_ptr };
                                    let tsc_freq = vdso.tsc_freq_hz.load(Ordering::Acquire);
                                    if tsc_freq > 0 {
                                        let tsc_mult = vdso.tsc_mult;
                                        let tsc_shift = vdso.tsc_shift;
                                        ((tsc_delta as u128 * tsc_mult as u128) >> tsc_shift) as u64
                                    } else { 0 }
                                } else { 0 }
                            };
                            let total_ns = cur.cpu_time_ns.saturating_add(ns);
                            Timespec {
                                tv_sec: (total_ns / 1_000_000_000) as i64,
                                tv_nsec: (total_ns % 1_000_000_000) as i64,
                            }
                        } else {
                            // Process CPU time: process's accumulated + in-flight slice from current thread
                            if let Some(ref proc) = cur.process {
                                let tsc_delta = tsc_now.wrapping_sub(cur.slice_start_tsc);
                                let ns = {
                                    let vdso_ptr = crate::vdso_page();
                                    if !vdso_ptr.is_null() {
                                        let vdso = unsafe { &*vdso_ptr };
                                        let tsc_freq = vdso.tsc_freq_hz.load(Ordering::Acquire);
                                        if tsc_freq > 0 {
                                            let tsc_mult = vdso.tsc_mult;
                                            let tsc_shift = vdso.tsc_shift;
                                            ((tsc_delta as u128 * tsc_mult as u128) >> tsc_shift) as u64
                                        } else { 0 }
                                    } else { 0 }
                                };
                                let proc_ns = proc.cpu_time_ns.load(Ordering::Relaxed);
                                let total_ns = proc_ns.saturating_add(ns);
                                Timespec {
                                    tv_sec: (total_ns / 1_000_000_000) as i64,
                                    tv_nsec: (total_ns % 1_000_000_000) as i64,
                                }
                            } else {
                                Timespec { tv_sec: 0, tv_nsec: 0 }
                            }
                        }
                    } else {
                        Timespec { tv_sec: 0, tv_nsec: 0 }
                    }
                } else {
                    Timespec { tv_sec: 0, tv_nsec: 0 }
                };
                ts
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                Timespec { tv_sec: 0, tv_nsec: 0 }
            }
        }
        // SKYIOUS-native clocks
        CLOCK_SKYIOUS_THREAD_CPUTIME => {
            // Thread CPU time via vDSO page — zero-syscall fast-path for userspace
            // Reads thread_cputime_ns directly from vDSO page (no seqlock needed)
            #[cfg(target_arch = "x86_64")]
            {
                let vdso_ptr = crate::vdso_page();
                if !vdso_ptr.is_null() {
                    let vdso = unsafe { &*vdso_ptr };
                    let ns = vdso.thread_cputime_ns.load(Ordering::Acquire);
                    Timespec {
                        tv_sec: (ns / 1_000_000_000) as i64,
                        tv_nsec: (ns % 1_000_000_000) as i64,
                    }
                } else {
                    Timespec { tv_sec: 0, tv_nsec: 0 }
                }
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                Timespec { tv_sec: 0, tv_nsec: 0 }
            }
        }
        CLOCK_SKYIOUS_SCHED_DEADLINE => {
            // Scheduler deadline parameters for current thread (EDF)
            // Returns absolute deadline in tv_sec (ns since boot), runtime in tv_nsec
            #[cfg(target_arch = "x86_64")]
            {
                use crate::task::scheduler::{this_cpu_sched, is_deadline};
                
                let ts = if let Some(sched) = this_cpu_sched().try_lock() {
                    if let Some(ref cur) = sched.current_thread {
                        if is_deadline(cur.policy) {
                            Timespec {
                                tv_sec: cur.sched_deadline as i64, // absolute deadline (ns since boot)
                                tv_nsec: cur.sched_runtime as i64, // runtime budget (ns)
                            }
                        } else {
                            Timespec { tv_sec: 0, tv_nsec: 0 }
                        }
                    } else {
                        Timespec { tv_sec: 0, tv_nsec: 0 }
                    }
                } else {
                    Timespec { tv_sec: 0, tv_nsec: 0 }
                };
                ts
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                Timespec { tv_sec: 0, tv_nsec: 0 }
            }
        }
        CLOCK_SKYIOUS_BOOT_NS => {
            // Raw TSC nanoseconds since boot (no seqlock, read-only after init)
            // Fast path: inline TSC read + fixed-point multiply-shift
            #[cfg(target_arch = "x86_64")]
            {
                let tsc_now = {
                    let (lo, hi): (u32, u32);
                    unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi) };
                    ((hi as u64) << 32) | (lo as u64)
                };
                let vdso_ptr = crate::vdso_page();
                if !vdso_ptr.is_null() {
                    let vdso = unsafe { &*vdso_ptr };
                    let tsc_freq = vdso.tsc_freq_hz.load(Ordering::Acquire);
                    if tsc_freq > 0 {
                        let tsc_mult = vdso.tsc_mult;
                        let tsc_shift = vdso.tsc_shift;
                        let tsc_base = vdso.tsc_base.load(Ordering::Acquire);
                        let elapsed = tsc_now.wrapping_sub(tsc_base);
                        let ns = ((elapsed as u128 * tsc_mult as u128) >> tsc_shift) as u64;
                        Timespec {
                            tv_sec: (ns / 1_000_000_000) as i64,
                            tv_nsec: (ns % 1_000_000_000) as i64,
                        }
                    } else {
                        Timespec { tv_sec: 0, tv_nsec: 0 }
                    }
                } else {
                    Timespec { tv_sec: 0, tv_nsec: 0 }
                }
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                Timespec { tv_sec: 0, tv_nsec: 0 }
            }
        }
        CLOCK_SKYIOUS_KERNEL_FLAGS => {
            // Kernel feature flags via vDSO page — read-only after init
            // Returns bitfield of kernel capabilities
            #[cfg(target_arch = "x86_64")]
            {
                let vdso_ptr = crate::vdso_page();
                if !vdso_ptr.is_null() {
                    let vdso = unsafe { &*vdso_ptr };
                    let flags = vdso.kernel_flags.load(Ordering::Acquire);
                    Timespec {
                        tv_sec: (flags & 0xFFFFFFFF) as i64,
                        tv_nsec: (flags >> 32) as i64,
                    }
                } else {
                    Timespec { tv_sec: 0, tv_nsec: 0 }
                }
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                Timespec { tv_sec: 0, tv_nsec: 0 }
            }
        }
        CLOCK_SKYIOUS_PROCESS_INFO => {
            // Process info via vDSO page — cgroup ID, namespace flags, capabilities
            #[cfg(target_arch = "x86_64")]
            {
                let vdso_ptr = crate::vdso_page();
                if !vdso_ptr.is_null() {
                    let vdso = unsafe { &*vdso_ptr };
                    let cgroup_id = vdso.process_cgroup_id.load(Ordering::Acquire);
                    let namespace_flags = vdso.process_namespace_flags.load(Ordering::Acquire);
                    let cap_effective = vdso.process_cap_effective.load(Ordering::Acquire);
                    Timespec {
                        tv_sec: (cgroup_id & 0xFFFFFFFF) as i64,
                        tv_nsec: ((namespace_flags & 0xFFFFFFFF) as i64) | ((cap_effective & 0xFFFFFFFF) as i64) << 32,
                    }
                } else {
                    Timespec { tv_sec: 0, tv_nsec: 0 }
                }
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                Timespec { tv_sec: 0, tv_nsec: 0 }
            }
        }
        CLOCK_REALTIME_ALARM | CLOCK_BOOTTIME_ALARM => {
            // Alarm clocks - not implemented
            return errno::Errno::EINVAL as u64;
        }
        _ => return errno::Errno::EINVAL as u64,
    };
    0
}

pub fn sys_clock_gettime(clock_id: u64, tp: *mut Timespec) -> u64 {
    if tp.is_null() {
        return errno::Errno::EFAULT as u64;
    }
    const CLOCK_REALTIME: u64 = 0;
    const CLOCK_MONOTONIC: u64 = 1;
    const CLOCK_PROCESS_CPUTIME_ID: u64 = 2;
    const CLOCK_THREAD_CPUTIME_ID: u64 = 3;
    const CLOCK_MONOTONIC_RAW: u64 = 4;
    const CLOCK_REALTIME_COARSE: u64 = 5;
    const CLOCK_MONOTONIC_COARSE: u64 = 6;
    const CLOCK_BOOTTIME: u64 = 7;
    const CLOCK_REALTIME_ALARM: u64 = 8;
    const CLOCK_BOOTTIME_ALARM: u64 = 9;
    // SKYIOUS-native clocks
    const CLOCK_SKYIOUS_THREAD_CPUTIME: u64 = 100;
    const CLOCK_SKYIOUS_SCHED_DEADLINE: u64 = 101;
    const CLOCK_SKYIOUS_BOOT_NS: u64 = 102;
    const CLOCK_SKYIOUS_KERNEL_FLAGS: u64 = 103;
    const CLOCK_SKYIOUS_PROCESS_INFO: u64 = 104;

    // Fast path: try vDSO TSC-based time for CLOCK_MONOTONIC and CLOCK_REALTIME (x86_64 only)
    #[cfg(target_arch = "x86_64")]
    if matches!(clock_id, CLOCK_MONOTONIC | CLOCK_REALTIME) {
        let (sec, nsec) = crate::vdso_read_tsc_time(clock_id);
        if sec != 0 || nsec != 0 {
            let ts = Timespec { tv_sec: sec as i64, tv_nsec: nsec as i64 };
            if unsafe {
                user_access::copy_to_user(
                    tp as *mut u8,
                    core::slice::from_raw_parts(
                        &ts as *const _ as *const u8,
                        core::mem::size_of::<Timespec>(),
                    ),
                )
            }
            .is_ok()
            {
                return 0;
            }
            return errno::Errno::EFAULT as u64;
        }
    }

    // Fallback: slow path
    let ts = match clock_id {
        CLOCK_REALTIME => {
            let (sec, nsec) = crate::drivers::rtc::read_realtime();
            Timespec {
                tv_sec: sec,
                tv_nsec: nsec,
            }
        }
        CLOCK_MONOTONIC => {
            let ticks = crate::interrupts::get_ticks();
            let total_ms = ticks * 10;
            Timespec {
                tv_sec: (total_ms / 1000) as i64,
                tv_nsec: ((total_ms % 1000) * 1_000_000) as i64,
            }
        }
        CLOCK_MONOTONIC_RAW => {
            // Same as monotonic but without NTP adjustments - use tick-based
            let ticks = crate::interrupts::get_ticks();
            let total_ms = ticks * 10;
            Timespec {
                tv_sec: (total_ms / 1000) as i64,
                tv_nsec: ((total_ms % 1000) * 1_000_000) as i64,
            }
        }
        CLOCK_BOOTTIME => {
            // Monotonic time including suspend - use tick-based
            let ticks = crate::interrupts::get_ticks();
            let total_ms = ticks * 10;
            Timespec {
                tv_sec: (total_ms / 1000) as i64,
                tv_nsec: ((total_ms % 1000) * 1_000_000) as i64,
            }
        }
        CLOCK_REALTIME_COARSE | CLOCK_MONOTONIC_COARSE => {
            // Low-resolution versions - round to 1ms
            let ticks = crate::interrupts::get_ticks();
            let total_ms = ticks * 10;
            let coarse_sec = (total_ms / 1000) as i64;
            Timespec {
                tv_sec: if clock_id == CLOCK_REALTIME_COARSE {
                    coarse_sec
                } else {
                    coarse_sec
                },
                tv_nsec: 0,
            }
        }
        CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            // Process/thread CPU time — read from scheduler + in-flight slice
            #[cfg(target_arch = "x86_64")]
            {
                use crate::task::scheduler::this_cpu_sched;
                use core::sync::atomic::Ordering;
                
                let tsc_now = {
                    let (lo, hi): (u32, u32);
                    unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi) };
                    ((hi as u64) << 32) | (lo as u64)
                };
                
                let ts = if let Some(sched) = this_cpu_sched().try_lock() {
                    if let Some(ref cur) = sched.current_thread {
                        if clock_id == CLOCK_THREAD_CPUTIME_ID {
                            // Thread CPU time: thread's accumulated + in-flight slice
                            let tsc_delta = tsc_now.wrapping_sub(cur.slice_start_tsc);
                            let ns = {
                                let vdso_ptr = crate::vdso_page();
                                if !vdso_ptr.is_null() {
                                    let vdso = unsafe { &*vdso_ptr };
                                    let tsc_freq = vdso.tsc_freq_hz.load(Ordering::Acquire);
                                    if tsc_freq > 0 {
                                        let tsc_mult = vdso.tsc_mult;
                                        let tsc_shift = vdso.tsc_shift;
                                        ((tsc_delta as u128 * tsc_mult as u128) >> tsc_shift) as u64
                                    } else { 0 }
                                } else { 0 }
                            };
                            let total_ns = cur.cpu_time_ns.saturating_add(ns);
                            Timespec {
                                tv_sec: (total_ns / 1_000_000_000) as i64,
                                tv_nsec: (total_ns % 1_000_000_000) as i64,
                            }
                        } else {
                            // Process CPU time: process's accumulated + in-flight slice from current thread
                            if let Some(ref proc) = cur.process {
                                let tsc_delta = tsc_now.wrapping_sub(cur.slice_start_tsc);
                                let ns = {
                                    let vdso_ptr = crate::vdso_page();
                                    if !vdso_ptr.is_null() {
                                        let vdso = unsafe { &*vdso_ptr };
                                        let tsc_freq = vdso.tsc_freq_hz.load(Ordering::Acquire);
                                        if tsc_freq > 0 {
                                            let tsc_mult = vdso.tsc_mult;
                                            let tsc_shift = vdso.tsc_shift;
                                            ((tsc_delta as u128 * tsc_mult as u128) >> tsc_shift) as u64
                                        } else { 0 }
                                    } else { 0 }
                                };
                                let proc_ns = proc.cpu_time_ns.load(Ordering::Relaxed);
                                let total_ns = proc_ns.saturating_add(ns);
                                Timespec {
                                    tv_sec: (total_ns / 1_000_000_000) as i64,
                                    tv_nsec: (total_ns % 1_000_000_000) as i64,
                                }
                            } else {
                                Timespec { tv_sec: 0, tv_nsec: 0 }
                            }
                        }
                    } else {
                        Timespec { tv_sec: 0, tv_nsec: 0 }
                    }
                } else {
                    Timespec { tv_sec: 0, tv_nsec: 0 }
                };
                ts
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                Timespec { tv_sec: 0, tv_nsec: 0 }
            }
        }
        CLOCK_REALTIME_ALARM | CLOCK_BOOTTIME_ALARM => {
            // Alarm clocks - not implemented
            return errno::Errno::EINVAL as u64;
        }
        _ => return errno::Errno::EINVAL as u64,
    };
    if unsafe {
        user_access::copy_to_user(
            tp as *mut u8,
            core::slice::from_raw_parts(
                &ts as *const _ as *const u8,
                core::mem::size_of::<Timespec>(),
            ),
        )
    }
    .is_err()
    {
        return errno::Errno::EFAULT as u64;
    }
    0
}

pub fn sys_clock_getres(clock_id: u64, res: *mut Timespec) -> u64 {
    // Resolution via compile-time constant function
    let (sec, nsec) = clock_resolution(clock_id);
    if sec == 0 && nsec == 0 {
        return errno::Errno::EINVAL as u64;
    }
    let ts = Timespec { tv_sec: sec, tv_nsec: nsec };
    if !res.is_null() {
        if unsafe {
            user_access::copy_to_user(
                res as *mut u8,
                core::slice::from_raw_parts(
                    &ts as *const _ as *const u8,
                    core::mem::size_of::<Timespec>(),
                ),
            )
        }
        .is_err()
        {
            return errno::Errno::EFAULT as u64;
        }
    }
    0
}

pub fn sys_clock_nanosleep(
    clock_id: u64,
    flags: u64,
    req_ptr: *const Timespec,
    rem_ptr: *mut Timespec,
) -> u64 {
    const TIMER_ABSTIME: u64 = 1;
    const CLOCK_MONOTONIC: u64 = 1;

    if req_ptr.is_null() {
        return errno::Errno::EINVAL as u64;
    }

    let mut req = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe {
        user_access::copy_from_user(
            core::slice::from_raw_parts_mut(
                &mut req as *mut _ as *mut u8,
                core::mem::size_of::<Timespec>(),
            ),
            req_ptr as *const u8,
        )
    }
    .is_err()
    {
        return errno::Errno::EFAULT as u64;
    }

    if req.tv_sec < 0 || req.tv_nsec < 0 || req.tv_nsec >= 1_000_000_000 {
        return errno::Errno::EINVAL as u64;
    }

    if crate::syscalls::check_signal_interrupt() {
        if !rem_ptr.is_null() {
            let _ = unsafe {
                user_access::copy_to_user(
                    rem_ptr as *mut u8,
                    core::slice::from_raw_parts(
                        &req as *const _ as *const u8,
                        core::mem::size_of::<Timespec>(),
                    ),
                )
            };
        }
        return errno::Errno::EINTR as u64;
    }

    let req_ns = (req.tv_sec as u64) * 1_000_000_000 + (req.tv_nsec as u64);
    let ticks_per_ns: u64 = 10_000_000; // 100Hz
    let sleep_ticks = core::cmp::max(1, req_ns / ticks_per_ns);

    let target_tick = if (flags & TIMER_ABSTIME) != 0 && clock_id == CLOCK_MONOTONIC {
        // Absolute: ticks already represent monotonic time
        sleep_ticks
    } else {
        crate::interrupts::get_ticks() + sleep_ticks
    };

    {
        let mut sched = crate::task::scheduler::this_cpu_sched().lock();
        if let Some(current) = sched.current_thread.as_mut() {
            current.status = crate::task::thread::ThreadStatus::Blocked;
            current.sleep_until = Some(target_tick);
        }
    }

    crate::task::scheduler::schedule();

    // Return remaining time in rem if requested
    if !rem_ptr.is_null() {
        let zero = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let _ = unsafe {
            user_access::copy_to_user(
                rem_ptr as *mut u8,
                core::slice::from_raw_parts(
                    &zero as *const _ as *const u8,
                    core::mem::size_of::<Timespec>(),
                ),
            )
        };
    }
    0
}

pub fn sys_getpeername(sockfd: u64, addr: *mut u8, addrlen: *mut u32) -> u64 {
    // AF_UNIX check
    {
        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let fd_table = process.files.lock().fd_table.clone();
            if (sockfd as usize) < fd_table.len() {
                if let Some(FileDescriptor::UnixSocket(handle, _stype)) = fd_table[sockfd as usize]
                {
                    drop(fd_table);
                    drop(process_lock);
                    return crate::net::unix::getpeername_unix(handle, addr, addrlen)
                        .map(|_| 0u64)
                        .unwrap_or_else(|e| e as u64);
                }
            }
        }
    }

    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        let process_lock = CURRENT_PROCESS.lock();
        let process = match *process_lock {
            Some(ref p) => p,
            None => return errno::Errno::ESRCH as u64,
        };
        let fd_table = process.files.lock().fd_table.clone();
        if (sockfd as usize) >= fd_table.len() {
            return errno::Errno::EBADF as u64;
        }

        if let Some(FileDescriptor::Socket(handle, stype)) = fd_table[sockfd as usize] {
            let mut sockets = crate::net::SOCKETS.lock();
            match stype {
                crate::task::process::SocketType::Tcp => {
                    if let Some(ep) =
                        with_tcp_mut(&mut sockets, handle, |socket| socket.remote_endpoint())
                            .flatten()
                    {
                        write_sockaddr(addr, addrlen, &ep);
                        return 0;
                    }
                    return errno::Errno::ENOTCONN as u64;
                }
                crate::task::process::SocketType::Udp => {
                    return errno::Errno::ENOTCONN as u64;
                }
                _ => return errno::Errno::EOPNOTSUPP as u64,
            }
        }
        errno::Errno::EBADF as u64
    }
}

#[cfg(not(feature = "net"))]
pub fn sys_resolve(_name_ptr: *const u8, _ip_ptr: *mut u8) -> u64 {
    errno::Errno::ENOSYS as u64
}

#[cfg(feature = "net")]
pub fn sys_resolve(name_ptr: *const u8, ip_ptr: *mut u8) -> u64 {
    let name_str = match unsafe { user_access::read_user_string(name_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };

    if let Some(smoltcp::wire::IpAddress::Ipv4(ipv4)) = crate::net::dns::resolve_hostname(&name_str)
    {
        let bytes = ipv4.as_bytes();
        if unsafe { user_access::copy_to_user(ip_ptr, bytes) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
        return 0;
    }

    errno::Errno::ENOENT as u64
}

pub fn sys_select(
    nfds: u64,
    readfds: *mut u64,
    writefds: *mut u64,
    exceptfds: *mut u64,
    timeout: *const u64,
) -> u64 {
    let process = match *CURRENT_PROCESS.lock() {
        Some(ref p) => p.clone(),
        None => return errno::Errno::ESRCH as u64,
    };
    let fd_table = process.files.lock().fd_table.clone();
    let mut ready_count;
    let deadline = if !timeout.is_null() {
        let mut tv_sec = 0u64;
        let mut tv_nsec = 0u64;
        unsafe {
            let _ = user_access::copy_from_user(
                core::slice::from_raw_parts_mut(&mut tv_sec as *mut _ as *mut u8, 8),
                timeout as *const u8,
            );
            let _ = user_access::copy_from_user(
                core::slice::from_raw_parts_mut(&mut tv_nsec as *mut _ as *mut u8, 8),
                timeout.add(8) as *const u8,
            );
        }
        let timeout_ms = tv_sec * 1000 + tv_nsec / 1_000_000;
        let now = crate::interrupts::get_ticks() * 10;
        if timeout_ms > 0 {
            now + timeout_ms / 10
        } else {
            0
        }
    } else {
        0
    };

    let mut poll_count = 0;
    loop {
        poll_count += 1;
        if poll_count > 1000 {
            break;
        }

        let mut read_set: u64 = 0;
        let mut write_set: u64 = 0;
        #[allow(unused_mut)]
        let mut except_set: u64 = 0;

        for fd in 0..core::cmp::min(nfds, 64) {
            if fd as usize >= fd_table.len() {
                continue;
            }
            let readable = match fd_table[fd as usize] {
                Some(ref desc) => match desc {
                    FileDescriptor::File { node, .. } => {
                        node.stat().map(|s| s.st_size > 0).unwrap_or(false)
                    }
                    FileDescriptor::Socket(_, _) => true,
                    FileDescriptor::UnixSocket(_, _) => true,
                    FileDescriptor::PtyMaster { pair, .. } => !pair.lock().master.buf.is_empty(),
                    FileDescriptor::PtySlave { pair, .. } => !pair.lock().slave.buf.is_empty(),
                    FileDescriptor::SignalFd(handle) => {
                        let fds = SIGNAL_FDS.lock();
                        fds.get(handle)
                            .map(|d| !d.lock().pending.is_empty())
                            .unwrap_or(false)
                    }
                    FileDescriptor::EventFd(data) => data.lock().counter > 0,
                    FileDescriptor::TimerFd(data) => data.lock().expirations > 0,
                    FileDescriptor::IoUringFd(data) => data
                        .lock()
                        .downcast_ref::<crate::syscalls::io_uring::IoUringInstance>()
                        .map_or(false, |i| i.peek_cqes() > 0),
                    FileDescriptor::InotifyFd { instance_key, .. } => {
                        crate::syscalls::inotify::inotify_has_events(*instance_key)
                    }
                    FileDescriptor::Mqueue(obj) => !obj.queue().is_empty(),
                },
                None => false,
            };
            let writable = fd_table[fd as usize].is_some();

            if readable {
                read_set |= 1 << fd;
            }
            if writable {
                write_set |= 1 << fd;
            }
        }

        let mut read_set_masked = read_set;
        let mut write_set_masked = write_set;
        let mut except_set_masked = except_set;

        if !readfds.is_null() {
            let mut user_set = 0u64;
            unsafe {
                let _ = user_access::copy_from_user(
                    core::slice::from_raw_parts_mut(&mut user_set as *mut _ as *mut u8, 8),
                    readfds as *const u8,
                );
            }
            read_set_masked &= user_set;
        }
        if !writefds.is_null() {
            let mut user_set = 0u64;
            unsafe {
                let _ = user_access::copy_from_user(
                    core::slice::from_raw_parts_mut(&mut user_set as *mut _ as *mut u8, 8),
                    writefds as *const u8,
                );
            }
            write_set_masked &= user_set;
        }
        if !exceptfds.is_null() {
            let mut user_set = 0u64;
            unsafe {
                let _ = user_access::copy_from_user(
                    core::slice::from_raw_parts_mut(&mut user_set as *mut _ as *mut u8, 8),
                    exceptfds as *const u8,
                );
            }
            except_set_masked &= user_set;
        }

        ready_count = read_set_masked.count_ones() as u64
            + write_set_masked.count_ones() as u64
            + except_set_masked.count_ones() as u64;

        if ready_count > 0 {
            if !readfds.is_null() {
                unsafe {
                    let _ = user_access::copy_to_user(
                        readfds as *mut u8,
                        core::slice::from_raw_parts(&read_set_masked as *const _ as *const u8, 8),
                    );
                }
            }
            if !writefds.is_null() {
                unsafe {
                    let _ = user_access::copy_to_user(
                        writefds as *mut u8,
                        core::slice::from_raw_parts(&write_set_masked as *const _ as *const u8, 8),
                    );
                }
            }
            if !exceptfds.is_null() {
                unsafe {
                    let _ = user_access::copy_to_user(
                        exceptfds as *mut u8,
                        core::slice::from_raw_parts(&except_set_masked as *const _ as *const u8, 8),
                    );
                }
            }
            return ready_count;
        }

        if !timeout.is_null() {
            let ticks = crate::interrupts::get_ticks() * 10;
            if deadline > 0 && ticks >= deadline {
                break;
            }
        }
        crate::task::scheduler::try_schedule();
    }

    if !readfds.is_null() {
        unsafe {
            let _ = user_access::copy_to_user(readfds as *mut u8, &0u64.to_ne_bytes());
        }
    }
    if !writefds.is_null() {
        unsafe {
            let _ = user_access::copy_to_user(writefds as *mut u8, &0u64.to_ne_bytes());
        }
    }
    if !exceptfds.is_null() {
        unsafe {
            let _ = user_access::copy_to_user(exceptfds as *mut u8, &0u64.to_ne_bytes());
        }
    }
    0
}

pub fn sys_poll(fds: *const u8, nfds: usize, timeout_ms: i32) -> u64 {
    if nfds > 256 {
        return errno::Errno::ENOMEM as u64;
    }
    if fds.is_null() {
        return errno::Errno::EFAULT as u64;
    }

    let process = match *CURRENT_PROCESS.lock() {
        Some(ref p) => p.clone(),
        None => return errno::Errno::ESRCH as u64,
    };

    // Copy pollfd array from userspace
    let mut poll_fds: alloc::vec::Vec<(i32, i16, i16)> = alloc::vec::Vec::with_capacity(nfds);
    for i in 0..nfds {
        let mut buf = [0u8; 8];
        unsafe {
            if user_access::copy_from_user(&mut buf, fds.add(i * 8)).is_err() {
                return errno::Errno::EFAULT as u64;
            }
        }
        let fd = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let events = i16::from_ne_bytes([buf[4], buf[5]]);
        poll_fds.push((fd, events, 0i16));
    }

    let deadline = if timeout_ms > 0 {
        let now = crate::interrupts::get_ticks() * 10;
        Some(now + timeout_ms as u64 / 10)
    } else {
        None
    };

    let mut poll_count = 0;
    loop {
        poll_count += 1;
        if poll_count > 1000 {
            break;
        }

        let fd_table = process.files.lock().fd_table.clone();
        let mut ready = 0usize;
        for (fd, events, revents) in poll_fds.iter_mut() {
            if *fd < 0 {
                continue;
            }
            *revents = 0;
            let desc = if (*fd as usize) < fd_table.len() {
                fd_table[*fd as usize].as_ref()
            } else {
                None
            };
            match desc {
                Some(FileDescriptor::File { node, .. }) => {
                    if *events & POLLIN != 0 && node.stat().map(|s| s.st_size > 0).unwrap_or(false)
                    {
                        *revents |= POLLIN;
                    }
                    if *events & POLLOUT != 0 {
                        *revents |= POLLOUT;
                    }
                }
                Some(FileDescriptor::Socket(_, _)) => {
                    if *events & POLLIN != 0 {
                        *revents |= POLLIN;
                    }
                    if *events & POLLOUT != 0 {
                        *revents |= POLLOUT;
                    }
                }
                Some(FileDescriptor::UnixSocket(handle, _)) => {
                    if *events & POLLIN != 0 && crate::net::unix::socket_has_data(*handle) {
                        *revents |= POLLIN;
                    }
                    if *events & POLLOUT != 0 {
                        *revents |= POLLOUT;
                    }
                }
                Some(FileDescriptor::PtyMaster { pair, .. }) => {
                    let buf = &pair.lock().master.buf;
                    if *events & POLLIN != 0 && !buf.is_empty() {
                        *revents |= POLLIN;
                    }
                    if *events & POLLOUT != 0 {
                        *revents |= POLLOUT;
                    }
                }
                Some(FileDescriptor::PtySlave { pair, .. }) => {
                    let buf = &pair.lock().slave.buf;
                    if *events & POLLIN != 0 && !buf.is_empty() {
                        *revents |= POLLIN;
                    }
                    if *events & POLLOUT != 0 {
                        *revents |= POLLOUT;
                    }
                }
                Some(FileDescriptor::SignalFd(handle)) => {
                    let has_pending = {
                        let fds = SIGNAL_FDS.lock();
                        fds.get(handle)
                            .map(|d| !d.lock().pending.is_empty())
                            .unwrap_or(false)
                    };
                    if *events & POLLIN != 0 && has_pending {
                        *revents |= POLLIN;
                    }
                    if *events & POLLOUT != 0 {
                        *revents |= POLLOUT;
                    }
                }
                Some(FileDescriptor::EventFd(data)) => {
                    let counter = data.lock().counter;
                    if *events & POLLIN != 0 && counter > 0 {
                        *revents |= POLLIN;
                    }
                    if *events & POLLOUT != 0 {
                        *revents |= POLLOUT;
                    }
                }
                Some(FileDescriptor::TimerFd(data)) => {
                    let expirations = data.lock().expirations;
                    if *events & POLLIN != 0 && expirations > 0 {
                        *revents |= POLLIN;
                    }
                }
                Some(FileDescriptor::IoUringFd(data)) => {
                    let pending = data
                        .lock()
                        .downcast_ref::<crate::syscalls::io_uring::IoUringInstance>()
                        .map_or(0, |i| i.peek_cqes());
                    if *events & POLLIN != 0 && pending > 0 {
                        *revents |= POLLIN;
                    }
                }
                Some(FileDescriptor::InotifyFd { instance_key, .. }) => {
                    if *events & POLLIN != 0
                        && crate::syscalls::inotify::inotify_has_events(*instance_key)
                    {
                        *revents |= POLLIN;
                    }
                }
                Some(FileDescriptor::Mqueue(obj)) => {
                    let attr = obj.get_attr();
                    if *events & POLLIN != 0 && attr.mq_curmsgs > 0 {
                        *revents |= POLLIN;
                    }
                    if *events & POLLOUT != 0 && attr.mq_curmsgs < attr.mq_maxmsg {
                        *revents |= POLLOUT;
                    }
                }
                None => {
                    *revents |= POLLNVAL;
                }
            }
            if *revents != 0 {
                ready += 1;
            }
        }
        drop(fd_table);

        if ready > 0 {
            for (i, (_fd, _events, revents)) in poll_fds.iter().enumerate() {
                let r = revents.to_ne_bytes();
                unsafe {
                    let _ = user_access::copy_to_user(fds.add(i * 8 + 4) as *mut u8, &r);
                }
            }
            return ready as u64;
        }

        if let Some(dl) = deadline {
            let ticks = crate::interrupts::get_ticks() * 10;
            if ticks >= dl {
                break;
            }
        } else if timeout_ms == 0 {
            break;
        }
        crate::task::scheduler::try_schedule();
    }

    // Timeout or nothing ready: write zero revents back
    for i in 0..nfds {
        let zero: [u8; 2] = [0, 0];
        unsafe {
            let _ = user_access::copy_to_user(fds.add(i * 8 + 4) as *mut u8, &zero);
        }
    }
    0
}

pub fn sys_reboot(magic: u64, cmd: u64) -> u64 {
    if magic != 0xDEAD_BEEF {
        return errno::Errno::EINVAL as u64;
    }
    // Only root or CAP_SYS_BOOT can reboot
    let euid = get_current_euid();
    if euid != 0 && !has_capability(CAP_SYS_BOOT) {
        audit_log("CAP_SYS_BOOT", "DENIED");
        return errno::Errno::EPERM as u64;
    }
    audit_log("REBOOT", if cmd == 0 { "poweroff" } else { "reboot" });
    match cmd {
        0 => {
            // Power off
            crate::println!("[SYSCALL] system poweroff");
            // Try ACPI S5 first, then fall back to QEMU-specific
            if *crate::acpi::PM1A_CNT_PORT.get().unwrap_or(&0) != 0 {
                crate::acpi::acpi_shutdown();
            }
            // QEMU-specific: isa-debug-exit at port 0xf4, exit code 0x10
            let mut port = x86_64::instructions::port::Port::<u32>::new(0xf4);
            unsafe {
                port.write(0x10);
            }
            let mut port2 = x86_64::instructions::port::Port::<u16>::new(0x604);
            unsafe {
                port2.write(0x2000);
            }
            x86_64::instructions::interrupts::disable();
            loop {
                x86_64::instructions::hlt();
            }
        }
        1 => {
            // Reboot
            crate::println!("[SYSCALL] system reboot");
            // Try ACPI reset first, fall back to legacy
            crate::acpi::acpi_reboot();
            x86_64::instructions::interrupts::disable();
            loop {
                x86_64::instructions::hlt();
            }
        }
        _ => errno::Errno::EINVAL as u64,
    }
}

pub fn sys_hash(
    hash_type: u64,
    password_ptr: *const u8,
    password_len: u64,
    salt_out_ptr: *mut u8,
    _iterations: u64,
) -> u64 {
    const HASH_SHA256_PBKDF2: u64 = 0;

    match hash_type {
        HASH_SHA256_PBKDF2 => {
            let pw_len = password_len as usize;
            if pw_len > 256 {
                return errno::Errno::EINVAL as u64;
            }
            let mut password = alloc::vec![0u8; pw_len];
            if pw_len > 0
                && unsafe { user_access::copy_from_user(&mut password, password_ptr).is_err() }
            {
                return errno::Errno::EFAULT as u64;
            }

            // salt_out_ptr points to a 48-byte buffer: [salt 16 | dk 32]
            let mut buf = [0u8; 48];
            if unsafe { user_access::copy_from_user(&mut buf[..16], salt_out_ptr).is_err() } {
                return errno::Errno::EFAULT as u64;
            }

            let iterations = if _iterations > 0 {
                _iterations as u32
            } else {
                10000
            };

            let mut dk = [0u8; 32];
            crate::crypto::pbkdf2(&password, &buf[..16], iterations, &mut dk);

            // Write back: salt (16) + dk (32) = 48 bytes
            buf[16..48].copy_from_slice(&dk);
            if unsafe { user_access::copy_to_user(salt_out_ptr, &buf).is_err() } {
                return errno::Errno::EFAULT as u64;
            }
            iterations as u64
        }
        _ => errno::Errno::ENOSYS as u64,
    }
}

pub fn sys_objmgr_enum(buf: *mut u8, len: usize) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let handles = process.enum_handles();
    let entry_size = core::mem::size_of::<(
        crate::objects::handle::HandleValue,
        crate::objects::ObjectTypeId,
        u64,
    )>();
    if entry_size == 0 {
        return 0;
    }
    let max_entries = len / entry_size;
    let count = core::cmp::min(handles.len(), max_entries);
    if count > 0 {
        let mut serialized = alloc::vec::Vec::with_capacity(count * entry_size);
        for &(hv, oti) in handles.iter().take(count) {
            serialized.extend_from_slice(&hv.to_le_bytes());
            serialized.extend_from_slice(&oti.0.to_le_bytes());
            serialized.extend_from_slice(&0u64.to_le_bytes());
        }
        if unsafe { crate::syscalls::user_access::copy_to_user(buf, &serialized) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }
    count as u64
}

pub fn sys_objmgr_audit(pid: u64, buf: *mut u8, len: usize) -> u64 {
    let audits = crate::objects::namespace::audit_by_pid(pid);
    let entry_size = core::mem::size_of::<(u64, u16, u64)>();
    if entry_size == 0 {
        return 0;
    }
    let max_entries = len / entry_size;
    let count = core::cmp::min(audits.len(), max_entries);
    if count > 0 {
        let mut serialized = alloc::vec::Vec::with_capacity(count * entry_size);
        for &(ref name, oti) in audits.iter().take(count) {
            let name_len = name.len() as u64;
            serialized.extend_from_slice(&name_len.to_le_bytes());
            serialized.extend_from_slice(&oti.0.to_le_bytes());
            serialized.extend_from_slice(&0u64.to_le_bytes());
        }
        if unsafe { crate::syscalls::user_access::copy_to_user(buf, &serialized) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }
    count as u64
}

// ── getrandom(2) ─────────────────────────────────────────────────

const GRND_NONBLOCK: u64 = 1;

/// sys_getrandom(buf, count, flags)
/// Fill `buf` with `count` random bytes. The entropy pool is always
/// initialized (seeded from RDTSC + RDRAND at boot), so GRND_NONBLOCK
/// never returns EAGAIN in practice.
pub fn sys_getrandom(buf: *mut u8, count: usize, flags: u64) -> u64 {
    if buf.is_null() {
        return errno::Errno::EINVAL as u64;
    }
    // bits 0-1 are the only valid flags
    if flags & !3 != 0 {
        return errno::Errno::EINVAL as u64;
    }
    let mut written = 0usize;
    let mut chunk = [0u8; 64];
    while written < count {
        let remaining = count - written;
        let to_fill = remaining.min(64);
        let mut i = 0;
        while i + 8 <= to_fill {
            let val = crate::crypto::GLOBAL_ENTROPY.get_u64();
            chunk[i..i + 8].copy_from_slice(&val.to_le_bytes());
            i += 8;
        }
        if i < to_fill {
            let val = crate::crypto::GLOBAL_ENTROPY.get_u64();
            let bytes = val.to_le_bytes();
            for j in i..to_fill {
                chunk[j] = bytes[j - i];
            }
        }
        let dest = unsafe { buf.add(written) };
        if unsafe { crate::syscalls::user_access::copy_to_user(dest, &chunk[..to_fill]) }.is_err() {
            if written == 0 {
                return errno::Errno::EFAULT as u64;
            }
            break;
        }
        written += to_fill;
    }
    written as u64
}
