//! Core I/O syscalls: read, write, lseek, brk, mmap, munmap, mprotect,
//! ioctl, getdents64, fallocate, sendfile.
//! Extracted from fs.rs to keep each module under 1k lines.

use super::errno;
use super::*;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS, SocketType};
use crate::vfs::{VFS, VfsNode};
use crate::sync::IrqSafeMutex as Mutex;
use alloc::sync::Arc;
use x86_64::structures::paging::PageTableFlags;

// ─── ioctl helper structs (must be at module level, not inside match) ─

#[repr(C)]
struct Winsize { ws_row: u16, ws_col: u16, ws_xpixel: u16, ws_ypixel: u16 }

#[repr(C)]
struct Termios { c_iflag: u32, c_oflag: u32, c_cflag: u32, c_lflag: u32, c_cc: [u8; 19] }

#[repr(C)]
struct LinuxDirent64 { d_ino: u64, d_off: u64, d_reclen: u16, d_type: u8 }

// ─── read ─────────────────────────────────────────────────────────

pub fn sys_read(fd: u64, buf: *mut u8, count: usize) -> u64 {
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, ref offset }) => {
            let mut off = offset.lock();
            let pos = *off;
            let data = match node.read(count) { Ok(d) => d, Err(_) => return errno::Errno::EIO as u64 };
            let len = core::cmp::min(data.len(), count);
            if len == 0 { return 0; }
            if unsafe { user_access::copy_to_user(buf, &data[..len]) }.is_err() { return errno::Errno::EFAULT as u64; }
            *off = pos + len;
            len as u64
        },
        Some(FileDescriptor::PtyMaster { ref pair, .. }) => {
            let mut data = alloc::vec![0u8; count];
            match crate::pty::pty_read_master(pair, &mut data) {
                Ok(n) => {
                    if n == 0 { return 0; }
                    if unsafe { user_access::copy_to_user(buf, &data[..n]) }.is_err() { return errno::Errno::EFAULT as u64; }
                    n as u64
                }
                Err(()) => if count > 0 { errno::Errno::EAGAIN as u64 } else { 0 }
            }
        },
        Some(FileDescriptor::PtySlave { ref pair, .. }) => {
            let mut data = alloc::vec![0u8; count];
            let ldisc = crate::pty::PtyLineDiscipline::default();
            match crate::pty::pty_read_slave(pair, &mut data, &ldisc) {
                Ok(n) => {
                    if n == 0 { return 0; }
                    if unsafe { user_access::copy_to_user(buf, &data[..n]) }.is_err() { return errno::Errno::EFAULT as u64; }
                    n as u64
                }
                Err(()) => if count > 0 { errno::Errno::EAGAIN as u64 } else { 0 }
            }
        },
        Some(FileDescriptor::Socket(handle, stype)) => {
            drop(fd_table);
            let mut recv_buf = alloc::vec![0u8; count];
            let mut sockets = crate::net::SOCKETS.lock();
            match super::net::recvfrom_internal(&mut sockets, handle, stype, &mut recv_buf) {
                Ok((n, _ep)) => {
                    if n == 0 { return 0; }
                    if unsafe { user_access::copy_to_user(buf, &recv_buf[..n]) }.is_err() { return errno::Errno::EFAULT as u64; }
                    n as u64
                }
                Err(e) => e,
            }
        },
        Some(FileDescriptor::UnixSocket(handle, _)) => {
            drop(fd_table);
            let mut recv_buf = alloc::vec![0u8; count];
            match crate::net::unix::recvfrom_unix(handle, recv_buf.as_mut_ptr() as *mut u8, count as u64, core::ptr::null_mut(), core::ptr::null_mut()) {
                Ok(n) => { if unsafe { user_access::copy_to_user(buf, &recv_buf[..n as usize]) }.is_err() { return errno::Errno::EFAULT as u64; } n }
                Err(e) => e as u64,
            }
        },
        Some(FileDescriptor::SignalFd(handle)) => {
            drop(fd_table);
            let sig_fds = super::SIGNAL_FDS.lock();
            if let Some(data) = sig_fds.get(&handle) {
                let mut d = data.lock();
                if let Some(info) = d.pending.pop_front() {
                    let sig_info = crate::task::process::SignalFdInfo { signo: info.signo, pid: info.pid, uid: info.uid };
                    let bytes = unsafe { core::slice::from_raw_parts(&sig_info as *const _ as *const u8, core::mem::size_of::<crate::task::process::SignalFdInfo>()) };
                    if unsafe { user_access::copy_to_user(buf, bytes) }.is_err() { return errno::Errno::EFAULT as u64; }
                    core::mem::size_of::<crate::task::process::SignalFdInfo>() as u64
                } else { errno::Errno::EAGAIN as u64 }
            } else { errno::Errno::EBADF as u64 }
        },
        Some(FileDescriptor::EventFd(ref data_arc)) => {
            let d_arc = data_arc.clone();
            drop(fd_table);
            let mut d = d_arc.lock();
            let val = d.counter;
            if d.semaphore { if val > 0 { d.counter -= 1; } else { return errno::Errno::EAGAIN as u64; } } else { d.counter = 0; }
            let bytes = val.to_ne_bytes();
            if unsafe { user_access::copy_to_user(buf, &bytes) }.is_err() { return errno::Errno::EFAULT as u64; }
            8
        },
        None => errno::Errno::EBADF as u64,
    }
}

// ─── write ────────────────────────────────────────────────────────

pub fn sys_write(fd: u64, buf: *const u8, count: usize) -> u64 {
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let mut data = alloc::vec![0u8; count];
    if unsafe { user_access::copy_from_user(&mut data, buf) }.is_err() { return errno::Errno::EFAULT as u64; }
    let fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, ref offset }) => {
            let mut off = offset.lock();
            let pos = *off;
            match node.write(&data) {
                Ok(()) => { *off = pos + count; count as u64 }
                Err(_) => errno::Errno::EIO as u64,
            }
        },
        Some(FileDescriptor::PtyMaster { ref pair, .. }) => {
            match crate::pty::pty_write_master(pair, &data) {
                Ok(n) => n as u64,
                Err(()) => errno::Errno::EAGAIN as u64,
            }
        },
        Some(FileDescriptor::PtySlave { ref pair, .. }) => {
            match crate::pty::pty_write_slave(pair, &data) {
                Ok(n) => n as u64,
                Err(()) => errno::Errno::EAGAIN as u64,
            }
        },
        Some(FileDescriptor::Socket(handle, stype)) => {
            drop(fd_table);
            let mut sockets = crate::net::SOCKETS.lock();
            super::net::sendto_internal(&mut sockets, handle, stype, &data, None)
        },
        Some(FileDescriptor::UnixSocket(handle, _)) => {
            drop(fd_table);
            match crate::net::unix::sendto_unix(handle, data.as_ptr() as *const u8, count as u64, core::ptr::null(), 0) { Ok(n) => n, Err(e) => e as u64 }
        },
        Some(FileDescriptor::EventFd(ref data_arc)) => {
            let d_arc = data_arc.clone();
            drop(fd_table);
            let mut d = d_arc.lock();
            let val = u64::from_ne_bytes(data[..8].try_into().unwrap_or([0; 8]));
            if val == u64::MAX { return errno::Errno::EINVAL as u64; }
            d.counter = d.counter.saturating_add(val);
            8
        },
        _ => errno::Errno::EINVAL as u64,
    }
}

// ─── lseek ────────────────────────────────────────────────────────

pub fn sys_lseek(fd: u64, offset: i64, whence: i32) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
    let fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, offset: ref file_off }) => {
            let stat = match node.stat() { Ok(s) => s, Err(_) => return errno::Errno::EIO as u64 };
            let cur = *file_off.lock();
            let new_pos = match whence {
                0 => offset as usize,
                1 => (cur as i64 + offset) as usize,
                2 => (stat.st_size as i64 + offset) as usize,
                _ => return errno::Errno::EINVAL as u64,
            };
            *file_off.lock() = new_pos;
            new_pos as u64
        },
        _ => errno::Errno::ESPIPE as u64,
    }
}

// ─── brk ──────────────────────────────────────────────────────────

pub fn sys_brk(addr: u64) -> u64 {
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let current_brk = process.memory.lock().brk;
    if addr == 0 { return current_brk; }
    if addr > current_brk {
        process.add_vma(crate::task::process::Vma { start: current_brk, end: addr, flags: PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE, _name: "brk", file_handle: None, file_offset: 0, is_shared: false, shm_id: None });
    }
    process.memory.lock().brk = addr;
    addr
}

// ─── mmap / munmap / mprotect ─────────────────────────────────────

pub fn sys_mmap(addr: u64, len: u64, prot: u64, flags: u64, fd: u64, offset: u64) -> u64 {
    let page_size: u64 = 4096;
    let len = (len + page_size - 1) & !(page_size - 1);
    if len == 0 { return errno::Errno::EINVAL as u64; }
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    // Cgroup memory.max check
    {
        let cg_path = process.cgroup_path.lock();
        let hierarchy = crate::syscalls::cgroup::cgroup_ensure();
        if let Some(cg) = hierarchy.find_cgroup(&cg_path) {
            if !cg.can_allocate(len) {
                return errno::Errno::ENOMEM as u64;
            }
        }
    }
    let alloc_addr = if addr == 0 {
        let brk = process.memory.lock().brk;
        let align = 0x1000_0000u64;
        (brk + align - 1) & !(align - 1)
    } else { addr };
    let mut page_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if prot & 2 != 0 { page_flags |= PageTableFlags::WRITABLE; }
    if prot & 4 == 0 { page_flags |= PageTableFlags::NO_EXECUTE; }
    let is_shared = flags & 0x02 != 0;
    process.add_vma(crate::task::process::Vma { start: alloc_addr, end: alloc_addr + len, flags: page_flags, _name: "mmap", file_handle: if fd != u64::MAX { Some(fd) } else { None }, file_offset: offset, is_shared, shm_id: None });
    alloc_addr
}

pub fn sys_munmap(addr: u64, len: u64) -> u64 {
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    process.remove_vma_range(addr, addr + len);
    0
}

pub fn sys_mprotect(addr: u64, len: u64, prot: u64) -> u64 {
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let page_size: u64 = 4096;
    let aligned_end = (addr + len + page_size - 1) & !(page_size - 1);
    let aligned_addr = addr & !(page_size - 1);
    let mut new_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if prot & 2 != 0 { new_flags |= PageTableFlags::WRITABLE; }
    if prot & 4 == 0 { new_flags |= PageTableFlags::NO_EXECUTE; }
    let mut vmas = process.memory.lock().vmas.clone();
    for vma in vmas.iter_mut() {
        if vma.start < aligned_end && vma.end > aligned_addr {
            vma.flags = new_flags;
        }
    }
    0
}

// ─── getdents64 ───────────────────────────────────────────────────

pub fn sys_getdents64(fd: u64, buf: *mut u8, len: usize) -> u64 {
    let _vfs = VFS.lock();
    let proc = CURRENT_PROCESS.lock();
    let node = if let Some(ref p) = *proc {
        let fd_table = p.files.lock().fd_table.clone();
        if let Some(Some(FileDescriptor::File { node, .. })) = fd_table.get(fd as usize) { node.clone() } else { return errno::Errno::EBADF as u64; }
    } else { return errno::Errno::EBADF as u64 };
    if !node.is_dir() { return errno::Errno::ENOTDIR as u64; }
    drop(proc);
    let children = match node.children() { Ok(c) => c, Err(_) => return errno::Errno::EIO as u64 };
    let mut written: usize = 0;
    for child in &children {
        let name = child.name();
        let name_bytes = name.as_bytes();
        let reclen = ((core::mem::size_of::<u64>() * 3).saturating_add(name_bytes.len()).saturating_add(1 + 7)) & !7;
        if written + reclen > len { break; }
        let entry_offset = written;
        let d_type = if child.is_dir() { 4u8 } else { 8u8 };
        let dirent = LinuxDirent64 { d_ino: 1, d_off: (written + reclen) as u64, d_reclen: reclen as u16, d_type };
        let dirent_bytes = unsafe { core::slice::from_raw_parts(&dirent as *const _ as *const u8, core::mem::size_of::<LinuxDirent64>()) };
        if unsafe { buf.add(entry_offset) }.is_null() { return errno::Errno::EFAULT as u64; }
        unsafe { if user_access::copy_to_user(buf.add(entry_offset), dirent_bytes).is_err() { return errno::Errno::EFAULT as u64; } }
        let name_offset = entry_offset + core::mem::size_of::<LinuxDirent64>();
        unsafe { if user_access::copy_to_user(buf.add(name_offset), name_bytes).is_err() { return errno::Errno::EFAULT as u64; } }
        if name_offset + name_bytes.len() < entry_offset + reclen {
            let null_byte = [0u8];
            unsafe { if user_access::copy_to_user(buf.add(name_offset + name_bytes.len()), &null_byte).is_err() { return errno::Errno::EFAULT as u64; } }
        }
        written += reclen;
    }
    written as u64
}

// ─── ioctl ────────────────────────────────────────────────────────

pub fn sys_ioctl(fd: u64, request: u64, argp: *mut u8) -> u64 {
    const TIOCGWINSZ: u64 = 0x5413;
    const TCGETS: u64 = 0x5401;
    const TCSETS: u64 = 0x5402;
    const FIONBIO: u64 = 0x5421;
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
    match fd_table[fd as usize] {
        Some(FileDescriptor::PtyMaster { .. }) | Some(FileDescriptor::PtySlave { .. }) => {
            match request {
                TIOCGWINSZ => {
                    let ws = Winsize { ws_row: 24, ws_col: 80, ws_xpixel: 640, ws_ypixel: 400 };
                    if unsafe { user_access::copy_to_user(argp, core::slice::from_raw_parts(&ws as *const _ as *const u8, core::mem::size_of::<Winsize>())) }.is_err() { return errno::Errno::EFAULT as u64; }
                    0
                },
                TCGETS => {
                    let t = Termios { c_iflag: 0, c_oflag: 0, c_cflag: 0x800 | 0x32, c_lflag: 0x8a3b, c_cc: [0; 19] };
                    if unsafe { user_access::copy_to_user(argp, core::slice::from_raw_parts(&t as *const _ as *const u8, core::mem::size_of::<Termios>())) }.is_err() { return errno::Errno::EFAULT as u64; }
                    0
                },
                TCSETS => { 0 },
                FIONBIO => { 0 },
                _ => errno::Errno::ENOTTY as u64,
            }
        },
        Some(FileDescriptor::File { ref node, .. }) => {
            match request {
                TIOCGWINSZ => {
                    let ws = Winsize { ws_row: 24, ws_col: 80, ws_xpixel: 640, ws_ypixel: 400 };
                    if unsafe { user_access::copy_to_user(argp, core::slice::from_raw_parts(&ws as *const _ as *const u8, core::mem::size_of::<Winsize>())) }.is_err() { return errno::Errno::EFAULT as u64; }
                    0
                },
                _ => { match node.ioctl(request, argp) { Ok(v) => v, Err(()) => errno::Errno::ENOTTY as u64 } }
            }
        },
        Some(FileDescriptor::Socket(_handle, _stype)) => {
            drop(fd_table);
            match request { 0x5421 => 0, _ => errno::Errno::ENOTTY as u64 }
        },
        _ => errno::Errno::ENOTTY as u64,
    }
}

// ─── fallocate / sendfile ─────────────────────────────────────────

pub fn sys_fallocate(fd: u64, mode: i32, offset: i64, len: i64) -> u64 {
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, .. }) => { match node.fallocate(mode, offset, len) { Ok(()) => 0, Err(_) => errno::Errno::ENOSPC as u64 } }
        _ => errno::Errno::EBADF as u64,
    }
}

pub fn sys_sendfile(out_fd: u64, in_fd: u64, offset_ptr: *mut u64, count: u64) -> u64 {
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let fd_table = process.files.lock().fd_table.clone();
    if (out_fd as usize) >= fd_table.len() || (in_fd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
    let start_offset = if !offset_ptr.is_null() { let slice = unsafe { core::slice::from_raw_parts(offset_ptr as *const u8, 8) }; u64::from_ne_bytes(slice.try_into().unwrap_or([0; 8])) } else { 0 };
    let buf_size = core::cmp::min(count, 4096) as usize;
    let mut buf = alloc::vec![0u8; buf_size];
    match fd_table[in_fd as usize] {
        Some(FileDescriptor::File { ref node, offset: ref in_off, .. }) => {
            let mut read_pos = if !offset_ptr.is_null() { start_offset } else { *in_off.lock() as u64 };
            let mut total: u64 = 0;
            while total < count {
                let n = match node.read(buf.len()) { Ok(bytes) => { let l = bytes.len(); buf[..l].copy_from_slice(&bytes); l } Err(_) => break };
                if n == 0 { break; }
                match fd_table[out_fd as usize] {
                    Some(FileDescriptor::File { ref node, .. }) => { let _ = node.write(&buf[..n]); }
                    _ => return errno::Errno::EBADF as u64,
                }
                if !offset_ptr.is_null() { read_pos += n as u64; } else { *in_off.lock() += n as usize; }
                total += n as u64;
            }
            if !offset_ptr.is_null() { let new_offset = read_pos.to_ne_bytes(); if unsafe { user_access::copy_to_user(offset_ptr as *mut u8, &new_offset) }.is_err() { return errno::Errno::EFAULT as u64; } }
            total as u64
        }
        _ => errno::Errno::EBADF as u64,
    }
}
