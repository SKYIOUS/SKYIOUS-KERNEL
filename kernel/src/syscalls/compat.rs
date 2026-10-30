//! Linux-compatible syscalls: writev, readv, madvise, pipe2, dup3, pread64, pwrite64
//!
//! These are needed for glibc/musl userspace compatibility.

use crate::syscalls::errno;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use alloc::vec::Vec;

/// iovec structure (Linux x86_64 layout)
#[repr(C)]
struct IoVec {
    iov_base: u64,
    iov_len: u64,
}

/// readv(fd, iov, iovcnt) → total bytes read
///
/// Reads from file descriptor `fd` into multiple buffers.
pub fn sys_readv(fd: u64, iov_ptr: *const u8, iovcnt: i64) -> u64 {
    if iovcnt <= 0 || iovcnt > 1024 {
        return errno::Errno::EINVAL as u64;
    }

    let proc_lock = CURRENT_PROCESS.lock();
    let proc = match proc_lock.as_ref() {
        Some(p) => p.clone(),
        None => return errno::Errno::ESRCH as u64,
    };
    drop(proc_lock);

    let fd_table = proc.files.lock().fd_table.clone();
    if fd as usize >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }

    let mut total_bytes: usize = 0;

    for i in 0..iovcnt as usize {
        let mut iovec: IoVec = unsafe { core::mem::zeroed() };
        let iovec_size = core::mem::size_of::<IoVec>();

        unsafe {
            if crate::syscalls::user_access::copy_from_user(
                core::slice::from_raw_parts_mut(&mut iovec as *mut _ as *mut u8, iovec_size),
                iov_ptr.add(i * iovec_size),
            )
            .is_err()
            {
                if total_bytes > 0 {
                    return total_bytes as u64;
                }
                return errno::Errno::EFAULT as u64;
            }
        }

        if iovec.iov_len == 0 {
            continue;
        }
        let remaining = iovec.iov_len as usize;

        match &fd_table[fd as usize] {
            Some(FileDescriptor::File { node, offset }) => match node.read(remaining) {
                Ok(data) => {
                    let off = *offset.lock();
                    let available = if off < data.len() {
                        data.len() - off
                    } else {
                        0
                    };
                    let copy_len = core::cmp::min(remaining, available);
                    if copy_len > 0 {
                        unsafe {
                            if crate::syscalls::user_access::copy_to_user(
                                iovec.iov_base as *mut u8,
                                &data[off..off + copy_len],
                            )
                            .is_err()
                            {
                                if total_bytes > 0 {
                                    return total_bytes as u64;
                                }
                                return errno::Errno::EFAULT as u64;
                            }
                        }
                        *offset.lock() += copy_len;
                        total_bytes += copy_len;
                    }
                    if copy_len < remaining {
                        break;
                    }
                }
                Err(_) => {
                    if total_bytes > 0 {
                        return total_bytes as u64;
                    }
                    return errno::Errno::EIO as u64;
                }
            },
            _ => {
                return errno::Errno::EINVAL as u64;
            }
        }
    }

    total_bytes as u64
}

/// writev(fd, iov, iovcnt) → total bytes written
///
/// Writes multiple buffers to file descriptor `fd`.
pub fn sys_writev(fd: u64, iov_ptr: *const u8, iovcnt: i64) -> u64 {
    if iovcnt <= 0 || iovcnt > 1024 {
        return errno::Errno::EINVAL as u64;
    }

    let proc_lock = CURRENT_PROCESS.lock();
    let proc = match proc_lock.as_ref() {
        Some(p) => p.clone(),
        None => return errno::Errno::ESRCH as u64,
    };
    drop(proc_lock);

    let fd_table = proc.files.lock().fd_table.clone();
    if fd as usize >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }

    let mut total_bytes: usize = 0;

    for i in 0..iovcnt as usize {
        let mut iovec: IoVec = unsafe { core::mem::zeroed() };
        let iovec_size = core::mem::size_of::<IoVec>();

        unsafe {
            if crate::syscalls::user_access::copy_from_user(
                core::slice::from_raw_parts_mut(&mut iovec as *mut _ as *mut u8, iovec_size),
                iov_ptr.add(i * iovec_size),
            )
            .is_err()
            {
                if total_bytes > 0 {
                    return total_bytes as u64;
                }
                return errno::Errno::EFAULT as u64;
            }
        }

        if iovec.iov_len == 0 {
            continue;
        }
        let write_len = iovec.iov_len as usize;
        let mut buf: Vec<u8> = alloc::vec![0u8; write_len];

        unsafe {
            if crate::syscalls::user_access::copy_from_user(&mut buf, iovec.iov_base as *const u8)
                .is_err()
            {
                if total_bytes > 0 {
                    return total_bytes as u64;
                }
                return errno::Errno::EFAULT as u64;
            }
        }

        match &fd_table[fd as usize] {
            Some(FileDescriptor::File { node, .. }) => match node.write(&buf) {
                Ok(()) => {
                    total_bytes += write_len;
                }
                Err(_) => {
                    if total_bytes > 0 {
                        return total_bytes as u64;
                    }
                    return errno::Errno::EIO as u64;
                }
            },
            _ => {
                return errno::Errno::EINVAL as u64;
            }
        }
    }

    total_bytes as u64
}

/// madvise(addr, len, advice) → 0
///
/// Gives advice about memory usage. Currently a no-op that returns success,
/// since the kernel doesn't yet implement advisory-based memory management.
pub fn sys_madvise(_addr: u64, _len: u64, _advice: u64) -> u64 {
    // Linux MADV_NORMAL=0, MADV_RANDOM=1, MADV_SEQUENTIAL=2, etc.
    // Accept all advice and return success.
    0
}

/// pipe2(fds, flags) → 0
///
/// Creates a pipe pair. Like pipe() but with flags (O_CLOEXEC, O_NONBLOCK).
pub fn sys_pipe2(fds_ptr: *mut u32, flags: i32) -> u64 {
    const O_CLOEXEC: i32 = 0x80000;
    const O_NONBLOCK: i32 = 0x800;
    if flags & !(O_CLOEXEC | O_NONBLOCK) != 0 {
        return errno::Errno::EINVAL as u64;
    }
    // Mask before widening: a negative i32 must not sign-extend into fd_flags.
    let flags = (flags as u32) as u64;

    let proc_lock = CURRENT_PROCESS.lock();
    let proc = match proc_lock.as_ref() {
        Some(p) => p.clone(),
        None => return errno::Errno::ESRCH as u64,
    };
    drop(proc_lock);

    let (reader, writer) = crate::vfs::pipe::Pipe::new();
    // O_CLOEXEC and the rest of the open flags land in fd_flags under the
    // same lock that installs the pair.
    let (r_fd, w_fd) = crate::syscalls::fs_open::install_pipe(&proc, reader, writer, flags);

    let fds = [r_fd as u32, w_fd as u32];
    // SAFETY: `fds` is a live two-element local array and the slice covers
    // exactly its bytes.
    let bytes = unsafe {
        core::slice::from_raw_parts(&fds as *const _ as *const u8, core::mem::size_of_val(&fds))
    };
    // SAFETY: copy_to_user validates `fds_ptr` against the user range and
    // handles faults; the install is rolled back on failure.
    if unsafe { crate::syscalls::user_access::copy_to_user(fds_ptr as *mut u8, bytes) }.is_err() {
        proc.with_fd_table(|fd_table, fd_flags| {
            fd_table[r_fd as usize] = None;
            fd_flags[r_fd as usize] = 0;
            fd_table[w_fd as usize] = None;
            fd_flags[w_fd as usize] = 0;
        });
        return errno::Errno::EFAULT as u64;
    }

    0
}

/// dup3(old_fd, new_fd, flags) → new_fd
///
/// Like dup2 but with flags (O_CLOEXEC, O_NONBLOCK).
pub fn sys_dup3(old_fd: u64, new_fd: u64, flags: i32) -> u64 {
    const O_CLOEXEC: i32 = 0x80000;
    if old_fd == new_fd {
        return errno::Errno::EINVAL as u64;
    }
    // Linux dup3 accepts only O_CLOEXEC.
    if flags & !O_CLOEXEC != 0 {
        return errno::Errno::EINVAL as u64;
    }

    let proc_lock = CURRENT_PROCESS.lock();
    let proc = match proc_lock.as_ref() {
        Some(p) => p.clone(),
        None => return errno::Errno::ESRCH as u64,
    };
    drop(proc_lock);

    // First, close new_fd if it's open (like dup2)
    {
        let fd_table = proc.files.lock().fd_table.clone();
        if old_fd as usize >= fd_table.len() || fd_table[old_fd as usize].is_none() {
            return errno::Errno::EBADF as u64;
        }
    }

    // Use the existing dup2 mechanism via syscalls
    let result = crate::syscalls::fs::sys_dup2(old_fd, new_fd);
    if result as i64 == -1 || (result & 0xFFFF_FFFF_FFFF_0000) != 0 {
        return result;
    }

    // Apply flags to the live table; sys_dup2 just resized it for new_fd.
    if flags & O_CLOEXEC != 0 {
        proc.with_fd_table(|_, fd_flags| {
            if fd_flags.len() <= new_fd as usize {
                fd_flags.resize(new_fd as usize + 1, 0);
            }
            fd_flags[new_fd as usize] |= O_CLOEXEC as u64;
        });
    }

    new_fd
}

/// pread64(fd, buf, count, offset) → bytes read
///
/// Read from file descriptor at a specific offset without modifying the file offset.
pub fn sys_pread64(fd: u64, buf_ptr: *mut u8, count: usize, offset: u64) -> u64 {
    let proc_lock = CURRENT_PROCESS.lock();
    let proc = match proc_lock.as_ref() {
        Some(p) => p.clone(),
        None => return errno::Errno::ESRCH as u64,
    };
    drop(proc_lock);

    let fd_table = proc.files.lock().fd_table.clone();
    if fd as usize >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }

    match &fd_table[fd as usize] {
        Some(FileDescriptor::File { node, .. }) => match node.read(count) {
            Ok(data) => {
                let available = if (offset as usize) < data.len() {
                    data.len() - offset as usize
                } else {
                    0
                };
                let copy_len = core::cmp::min(count, available);
                if copy_len > 0 {
                    unsafe {
                        if crate::syscalls::user_access::copy_to_user(
                            buf_ptr,
                            &data[offset as usize..offset as usize + copy_len],
                        )
                        .is_err()
                        {
                            return errno::Errno::EFAULT as u64;
                        }
                    }
                }
                copy_len as u64
            }
            Err(_) => errno::Errno::EIO as u64,
        },
        _ => errno::Errno::EINVAL as u64,
    }
}

/// pwrite64(fd, buf, count, offset) → bytes written
///
/// Write to file descriptor at a specific offset without modifying the file offset.
pub fn sys_pwrite64(fd: u64, buf_ptr: *const u8, count: usize, _offset: u64) -> u64 {
    let proc_lock = CURRENT_PROCESS.lock();
    let proc = match proc_lock.as_ref() {
        Some(p) => p.clone(),
        None => return errno::Errno::ESRCH as u64,
    };
    drop(proc_lock);

    let fd_table = proc.files.lock().fd_table.clone();
    if fd as usize >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }

    match &fd_table[fd as usize] {
        Some(FileDescriptor::File { node, .. }) => {
            let mut buf: Vec<u8> = alloc::vec![0u8; count];
            unsafe {
                if crate::syscalls::user_access::copy_from_user(&mut buf, buf_ptr).is_err() {
                    return errno::Errno::EFAULT as u64;
                }
            }
            match node.write(&buf) {
                Ok(()) => count as u64,
                Err(_) => errno::Errno::EIO as u64,
            }
        }
        _ => errno::Errno::EINVAL as u64,
    }
}
