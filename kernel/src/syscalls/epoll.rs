//! epoll — efficient I/O event notification.
//!
//! Implements epoll_create1, epoll_ctl, and epoll_wait for event-driven I/O.
//! Each epoll instance is backed by a file descriptor that monitors a set of
//! other file descriptors for readiness events (EPOLLIN, EPOLLOUT, etc.).

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::sync::IrqSafeMutex as Mutex;
use crate::task::process::{CURRENT_PROCESS, FileDescriptor};
use crate::syscalls::errno;

// ── epoll event flags (Linux-compatible) ─────────────────────────────
pub const EPOLLIN: u32 = 0x001;
pub const EPOLLPRI: u32 = 0x002;
pub const EPOLLOUT: u32 = 0x004;
pub const EPOLLERR: u32 = 0x008;
pub const EPOLLHUP: u32 = 0x010;
pub const EPOLLRDNORM: u32 = 0x040;
pub const EPOLLRDBAND: u32 = 0x080;
pub const EPOLLWRNORM: u32 = 0x100;
pub const EPOLLWRBAND: u32 = 0x200;
pub const EPOLLMSG: u32 = 0x400;
pub const EPOLLRDHUP: u32 = 0x2000;
pub const EPOLLEXCLUSIVE: u32 = 0x10000000;
pub const EPOLLWAKEUP: u32 = 0x20000000;
pub const EPOLLONESHOT: u32 = 0x40000000;

// epoll_ctl commands
pub const EPOLL_CTL_ADD: i32 = 1;
pub const EPOLL_CTL_MOD: i32 = 2;
pub const EPOLL_CTL_DEL: i32 = 3;

// epoll_create1 flags
pub const EPOLL_CLOEXEC: i32 = 0x80000;

/// A single monitored file descriptor entry.
#[derive(Debug, Clone)]
pub struct EpollEntry {
    pub fd: i32,
    pub events: u32,
    pub data: u64, // user data (32-bit on Linux, but we use 64-bit for simplicity)
}

/// The epoll instance state — shared across clone/fork.
pub struct EpollInstance {
    /// Monitored file descriptors.
    pub entries: BTreeMap<i32, EpollEntry>,
    /// Whether the instance was created with EPOLL_CLOEXEC.
    pub cloexec: bool,
}

impl EpollInstance {
    pub fn new(cloexec: bool) -> Self {
        Self {
            entries: BTreeMap::new(),
            cloexec,
        }
    }
}

/// Global epoll instance table: epoll_fd → Arc<Mutex<EpollInstance>>
pub static EPOLL_INSTANCES: crate::sync::IrqSafeMutex<BTreeMap<u64, Arc<Mutex<EpollInstance>>>> =
    crate::sync::IrqSafeMutex::new(BTreeMap::new());

/// Allocate a new epoll file descriptor number.
fn alloc_epoll_fd() -> u64 {
    static NEXT_EPOLL_FD: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0x1000);
    NEXT_EPOLL_FD.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
}

/// epoll_create1(flags) → fd
///
/// Creates a new epoll instance. Returns a file descriptor that refers to
/// the new epoll instance.
pub fn sys_epoll_create1(flags: i32) -> u64 {
    let cloexec = (flags & EPOLL_CLOEXEC) != 0;
    let epoll_key = alloc_epoll_fd();
    let instance = Arc::new(Mutex::new(EpollInstance::new(cloexec)));

    EPOLL_INSTANCES.lock().insert(epoll_key, instance);

    // Register as a file descriptor in the current process
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref proc) = *lock {
        let mut fd_table = proc.files.lock().fd_table.clone();
        let fd_num = find_free_fd(&fd_table);
        if fd_num >= fd_table.len() {
            fd_table.resize(fd_num + 1, None);
        }
        // Use EventFd as a placeholder — epoll key is tracked in EPOLL_FD_MAP
        fd_table[fd_num] = Some(FileDescriptor::EventFd(Arc::new(crate::sync::IrqSafeMutex::new(
            crate::task::process::EventFdData {
                counter: epoll_key,
                semaphore: false,
                nonblock: false,
                key: 0, // epoll placeholder — not used for blocking
            },
        ))));
        // Map this fd to the epoll key
        EPOLL_FD_MAP.lock().insert((proc.id, fd_num as u64), epoll_key);
        fd_num as u64
    } else {
        EPOLL_INSTANCES.lock().remove(&epoll_key);
        errno::Errno::ESRCH as u64
    }
}

/// epoll_create(flags) → fd
/// Legacy wrapper around epoll_create1.
pub fn sys_epoll_create(flags: i32) -> u64 {
    sys_epoll_create1(flags)
}

/// Find the lowest available FD number in the table.
fn find_free_fd(fd_table: &[Option<FileDescriptor>]) -> usize {
    for (i, slot) in fd_table.iter().enumerate() {
        if slot.is_none() {
            return i;
        }
    }
    fd_table.len()
}

/// Global mapping from process FD numbers to epoll instance keys.
/// Key: (process_id, fd_num) → epoll_key
pub static EPOLL_FD_MAP: crate::sync::IrqSafeMutex<alloc::collections::BTreeMap<(u64, u64), u64>> =
    crate::sync::IrqSafeMutex::new(alloc::collections::BTreeMap::new());

/// Get the epoll key for an FD, if it's an epoll fd.
fn get_epoll_key_for_fd(proc_id: u64, fd: u64) -> Option<u64> {
    let map = EPOLL_FD_MAP.lock();
    map.get(&(proc_id, fd)).copied()
}

/// epoll_ctl(epfd, op, fd, event) → 0 on success
///
/// Control interface for an epoll instance.
/// - EPOLL_CTL_ADD: add a file descriptor to the interest list
/// - EPOLL_CTL_MOD: modify an existing file descriptor's events
/// - EPOLL_CTL_DEL: remove a file descriptor from the interest list
pub fn sys_epoll_ctl(epfd: u64, op: i32, fd: i32, event_ptr: *const u8) -> u64 {
    // Resolve the epoll key from the FD table
    let proc_lock = CURRENT_PROCESS.lock();
    let proc = match proc_lock.as_ref() {
        Some(p) => p.clone(),
        None => return errno::Errno::ESRCH as u64,
    };
    drop(proc_lock);

    let epoll_key = match get_epoll_key_for_fd(proc.id, epfd) {
        Some(k) => k,
        None => return errno::Errno::EBADF as u64,
    };

    let instances = EPOLL_INSTANCES.lock();
    let instance = match instances.get(&epoll_key) {
        Some(inst) => inst.clone(),
        None => {
            return errno::Errno::EBADF as u64;
        }
    };
    drop(instances);

    // Parse the epoll_event struct if provided
    let (events, data) = if !event_ptr.is_null() {
        let mut events: u32 = 0;
        let mut data: u64 = 0;
        unsafe {
            if crate::syscalls::user_access::copy_from_user(
                core::slice::from_raw_parts_mut(&mut events as *mut _ as *mut u8, 4),
                event_ptr,
            ).is_err() {
                return errno::Errno::EFAULT as u64;
            }
            if crate::syscalls::user_access::copy_from_user(
                core::slice::from_raw_parts_mut(&mut data as *mut _ as *mut u8, 8),
                event_ptr.add(8),
            ).is_err() {
                return errno::Errno::EFAULT as u64;
            }
        }
        (events, data)
    } else {
        (0u32, 0u64)
    };

    let mut inst = instance.lock();
    match op {
        EPOLL_CTL_ADD => {
            if inst.entries.contains_key(&fd) {
                return errno::Errno::EEXIST as u64;
            }
            inst.entries.insert(fd, EpollEntry { fd, events, data });
        }
        EPOLL_CTL_MOD => {
            if !inst.entries.contains_key(&fd) {
                return errno::Errno::ENOENT as u64;
            }
            inst.entries.insert(fd, EpollEntry { fd, events, data });
        }
        EPOLL_CTL_DEL => {
            if inst.entries.remove(&fd).is_none() {
                return errno::Errno::ENOENT as u64;
            }
        }
        _ => {
            return errno::Errno::EINVAL as u64;
        }
    }
    0
}

/// Perform a single readiness scan on the epoll instance.
/// Returns (ready_events, ready_data) for all ready FDs.
fn poll_readiness(
    instance: &alloc::sync::Arc<crate::sync::IrqSafeMutex<EpollInstance>>,
) -> (Vec<u32>, Vec<u64>) {
    let mut ready_events: Vec<u32> = Vec::new();
    let mut ready_data: Vec<u64> = Vec::new();

    let inst = instance.lock();
    let entries: alloc::vec::Vec<EpollEntry> = inst.entries.values().cloned().collect();
    drop(inst);

    let proc_lock = CURRENT_PROCESS.lock();
    let proc = match proc_lock.as_ref() {
        Some(p) => p.clone(),
        None => return (ready_events, ready_data),
    };
    drop(proc_lock);

    let fd_table = proc.files.lock().fd_table.clone();

    for entry in &entries {
        let fd = entry.fd;
        if fd < 0 || fd as usize >= fd_table.len() {
            continue;
        }

        if let Some(ref fd_entry) = fd_table[fd as usize] {
            let mut revents: u32 = 0;

            match fd_entry {
                FileDescriptor::File { node, .. } => {
                    let stat = node.stat().unwrap_or_default();
                    let mode = stat.st_mode;
                    if mode & 0o170000 == 0o100000 {
                        if (entry.events & EPOLLIN) != 0 {
                            revents |= EPOLLIN | EPOLLRDNORM;
                        }
                        if (entry.events & EPOLLOUT) != 0 {
                            revents |= EPOLLOUT | EPOLLWRNORM;
                        }
                    } else {
                        if (entry.events & EPOLLIN) != 0 {
                            revents |= EPOLLIN | EPOLLRDNORM;
                        }
                    }
                }
                FileDescriptor::Socket(..) | FileDescriptor::UnixSocket(..) => {
                    if (entry.events & EPOLLIN) != 0 {
                        revents |= EPOLLIN | EPOLLRDNORM;
                    }
                    if (entry.events & EPOLLOUT) != 0 {
                        revents |= EPOLLOUT | EPOLLWRNORM;
                    }
                }
                FileDescriptor::EventFd(data) => {
                    let counter = data.lock().counter;
                    if (entry.events & EPOLLIN) != 0 && counter > 0 {
                        revents |= EPOLLIN | EPOLLRDNORM;
                    }
                }
                FileDescriptor::TimerFd(data) => {
                    let expirations = data.lock().expirations;
                    if (entry.events & EPOLLIN) != 0 && expirations > 0 {
                        revents |= EPOLLIN | EPOLLRDNORM;
                    }
                }
                FileDescriptor::IoUringFd(data) => {
                    let pending = data.lock().peek_cqes();
                    if (entry.events & EPOLLIN) != 0 && pending > 0 {
                        revents |= EPOLLIN | EPOLLRDNORM;
                    }
                }
                FileDescriptor::SignalFd(..) => {
                    if (entry.events & EPOLLIN) != 0 {
                        revents |= EPOLLIN | EPOLLRDNORM;
                    }
                }
                _ => {
                    if (entry.events & EPOLLIN) != 0 {
                        revents |= EPOLLIN | EPOLLRDNORM;
                    }
                }
            }

            if (entry.events & EPOLLERR) != 0 {
                revents |= EPOLLERR;
            }
            if (entry.events & EPOLLHUP) != 0 {
                revents |= EPOLLHUP;
            }

            if revents != 0 {
                ready_events.push(revents);
                ready_data.push(entry.data);
            }
        }
    }
    drop(fd_table);
    (ready_events, ready_data)
}

/// epoll_wait(epfd, events, maxevents, timeout) → number of events
///
/// Waits for events on the epoll instance. Returns the number of file
/// descriptors that are ready for I/O.
///
/// timeout_ms semantics:
///   -1  = block indefinitely until at least one event is ready
///    0  = non-blocking: return immediately with whatever is ready
///   >0  = block for up to timeout_ms milliseconds
pub fn sys_epoll_wait(epfd: u64, events_ptr: *mut u8, maxevents: i32, timeout_ms: i32) -> u64 {
    if maxevents <= 0 {
        return errno::Errno::EINVAL as u64;
    }

    // Resolve the epoll key from the FD table
    let proc_lock = CURRENT_PROCESS.lock();
    let proc = match proc_lock.as_ref() {
        Some(p) => p.clone(),
        None => return errno::Errno::ESRCH as u64,
    };
    drop(proc_lock);

    let epoll_key = match get_epoll_key_for_fd(proc.id, epfd) {
        Some(k) => k,
        None => return errno::Errno::EBADF as u64,
    };

    let instances = EPOLL_INSTANCES.lock();
    let instance = match instances.get(&epoll_key) {
        Some(inst) => inst.clone(),
        None => {
            return errno::Errno::EBADF as u64;
        }
    };
    drop(instances);

    // Check for pending signals
    if crate::syscalls::check_signal_interrupt() {
        return errno::Errno::EINTR as u64;
    }

    // Poll for readiness, potentially blocking with timeout.
    //
    // For non-blocking (timeout_ms == 0), we do a single scan.
    // For blocking (timeout_ms == -1 or > 0), we loop: scan → sleep → rescan.
    // The scheduler wakes us when sleep_until expires or a signal arrives.
    let (mut ready_events, mut ready_data) = poll_readiness(&instance);

    if ready_events.is_empty() && timeout_ms != 0 {
        // Compute the deadline in ticks. Each tick ≈ 10 ms at 100 Hz.
        let deadline: Option<u64> = if timeout_ms < 0 {
            None // block indefinitely
        } else {
            // Convert ms to ticks (round up so we never under-sleep)
            let ticks = ((timeout_ms as u64) + 9) / 10;
            let now = crate::interrupts::get_ticks();
            Some(now.saturating_add(ticks))
        };

        // Spin-block loop: sleep → rescan until events or timeout.
        // Cap each sleep to at most 4 ticks (40 ms) to stay responsive
        // to external wakeups (e.g. a pipe writer signals readiness).
        const MAX_SLEEP_TICKS: u64 = 4;
        loop {
            let wake_tick = match deadline {
                Some(dl) => {
                    let now = crate::interrupts::get_ticks();
                    if now >= dl {
                        // Deadline already passed; do one final non-blocking scan.
                        break;
                    }
                    Some(core::cmp::min(dl, now + MAX_SLEEP_TICKS))
                }
                None => {
                    // Infinite wait — sleep for MAX_SLEEP_TICKS, then rescan.
                    let now = crate::interrupts::get_ticks();
                    Some(now + MAX_SLEEP_TICKS)
                }
            };

            // Park the current thread in the sleep queue.
            {
                let mut sched = crate::task::scheduler::this_cpu_sched().lock();
                if let Some(current) = sched.current_thread.as_mut() {
                    current.status = crate::task::thread::ThreadStatus::Blocked;
                    current.sleep_until = wake_tick;
                }
            }
            crate::task::scheduler::schedule();

            // Woken — check for signals first
            if crate::syscalls::check_signal_interrupt() {
                return errno::Errno::EINTR as u64;
            }

            // Re-check readiness
            let (ev, data) = poll_readiness(&instance);
            if !ev.is_empty() {
                ready_events = ev;
                ready_data = data;
                break;
            }

            // If timed out, do one final scan and return 0 if still empty
            if let Some(dl) = deadline {
                let now = crate::interrupts::get_ticks();
                if now >= dl {
                    let (ev2, data2) = poll_readiness(&instance);
                    ready_events = ev2;
                    ready_data = data2;
                    break;
                }
            }
        }
    }

    let count = core::cmp::min(ready_events.len(), maxevents as usize);

    // Write results to user space
    if count > 0 && !events_ptr.is_null() {
        for i in 0..count {
            let event_offset = i * 16; // sizeof(epoll_event) on x86_64 = 16 bytes
            let revents = ready_events[i];
            let data = ready_data[i];

            unsafe {
                let dst = events_ptr.add(event_offset);
                if crate::syscalls::user_access::copy_to_user(
                    dst,
                    core::slice::from_raw_parts(&revents as *const _ as *const u8, 4),
                ).is_err() {
                    return errno::Errno::EFAULT as u64;
                }
                if crate::syscalls::user_access::copy_to_user(
                    dst.add(4),
                    core::slice::from_raw_parts(&data as *const _ as *const u8, 8),
                ).is_err() {
                    return errno::Errno::EFAULT as u64;
                }
            }
        }
    }

    count as u64
}

/// epoll_pwait — epoll_wait with signal mask.
/// For now, delegates to epoll_wait (signal mask not yet implemented).
pub fn sys_epoll_pwait(
    epfd: u64,
    events_ptr: *mut u8,
    maxevents: i32,
    timeout_ms: i32,
    _sigmask_ptr: *const u8,
    _sigmask_size: usize,
) -> u64 {
    sys_epoll_wait(epfd, events_ptr, maxevents, timeout_ms)
}
