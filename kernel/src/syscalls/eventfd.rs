//! eventfd2 syscall implementation.
//!
//! eventfd creates a file descriptor that can be used for event notification
//! between userspace and kernel, or between threads/processes. The descriptor
//! refers to an underlying 64-bit unsigned integer counter.
//!
//! - write(): atomically adds a uint64 value to the counter
//! - read():  atomically returns the counter and resets to 0 (or decrements by 1
//!            in EFD_SEMAPHORE mode); blocks if counter == 0 unless EFD_NONBLOCK

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::task::process::{CURRENT_PROCESS, EventFdData, EFD_SEMAPHORE, EFD_NONBLOCK, EFD_CLOEXEC, EFD_MAX};
use crate::task::process::FileDescriptor;

/// Unique key generator for eventfd blocking/wake.
static NEXT_EFD_KEY: AtomicU64 = AtomicU64::new(0x4000_0000_0000);

fn next_key() -> u64 {
    NEXT_EFD_KEY.fetch_add(1, Ordering::Relaxed)
}

/// eventfd2(initval, flags) → fd
///
/// Creates a new eventfd file descriptor.
///
/// # Arguments
/// * `initval` - initial value of the counter
/// * `flags` - bitmask: EFD_SEMAPHORE (1), EFD_NONBLOCK (0x800), EFD_CLOEXEC (0x40000)
pub fn sys_eventfd2(initval: u32, flags: i32) -> u64 {
    let process = match *CURRENT_PROCESS.lock() {
        Some(ref p) => Arc::clone(p),
        None => return crate::syscalls::errno::Errno::ESRCH as u64,
    };

    let semaphore = (flags & EFD_SEMAPHORE) != 0;
    let nonblock = (flags & EFD_NONBLOCK) != 0;

    let data = Arc::new(crate::sync::IrqSafeMutex::new(EventFdData {
        counter: initval as u64,
        semaphore,
        nonblock,
        key: next_key(),
    }));

    let fd_obj = FileDescriptor::EventFd(data);
    let mut files = process.files.lock();
    let mut fd_table = files.fd_table.clone();
    let mut fd_flags = files.fd_flags.clone();
    let mut fd_num = None;
    for (i, slot) in fd_table.iter_mut().enumerate() {
        if slot.is_none() {
            fd_num = Some(i);
            break;
        }
    }
    let idx = match fd_num {
        Some(i) => {
            fd_table[i] = Some(fd_obj);
            i
        }
        None => {
            fd_table.push(Some(fd_obj));
            fd_table.len() - 1
        }
    };
    // Set FD_CLOEXEC if requested
    if (flags & EFD_CLOEXEC) != 0 {
        if idx >= fd_flags.len() {
            fd_flags.resize(idx + 1, 0);
        }
        fd_flags[idx] |= 0x80000; // FD_CLOEXEC
    }
    files.fd_table = fd_table;
    files.fd_flags = fd_flags;
    idx as u64
}
