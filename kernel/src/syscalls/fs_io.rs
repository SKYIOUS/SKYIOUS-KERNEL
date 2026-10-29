use super::errno;
use super::*;
use crate::syscalls::process_lifecycle::sys_fork;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::vfs::VFS;
use x86_64::structures::paging::PageTableFlags;

// ── msync / mremap / mincore constants ─────────────────────────────────

// msync flags
const MS_ASYNC: i32 = 1;
const MS_SYNC: i32 = 2;
const MS_INVALIDATE: i32 = 4;

// mremap flags
const MREMAP_MAYMOVE: i32 = 1;
const MREMAP_FIXED: i32 = 2;

// ─── ioctl helper structs (must be at module level, not inside match) ─

#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

#[repr(C)]
struct Termios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_cc: [u8; 19],
}

#[repr(C)]
struct LinuxDirent64 {
    d_ino: u64,
    d_off: u64,
    d_reclen: u16,
    d_type: u8,
}

// ─── read ─────────────────────────────────────────────────────────

pub fn sys_read(fd: u64, buf: *mut u8, count: usize) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }
    match fd_table[fd as usize] {
        Some(FileDescriptor::File {
            ref node,
            ref offset,
        }) => {
            let mut off = offset.lock();
            let pos = *off;
            let data = match node.read(count) {
                Ok(d) => d,
                Err(_) => return errno::Errno::EIO as u64,
            };
            let len = core::cmp::min(data.len(), count);
            if len == 0 {
                return 0;
            }
            if unsafe { user_access::copy_to_user(buf, &data[..len]) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            *off = pos + len;
            len as u64
        }
        Some(FileDescriptor::PtyMaster { ref pair, .. }) => {
            let mut data = alloc::vec![0u8; count];
            match crate::pty::pty_read_master(pair, &mut data) {
                Ok(n) => {
                    if n == 0 {
                        return 0;
                    }
                    if unsafe { user_access::copy_to_user(buf, &data[..n]) }.is_err() {
                        return errno::Errno::EFAULT as u64;
                    }
                    n as u64
                }
                Err(()) => {
                    if count > 0 {
                        errno::Errno::EAGAIN as u64
                    } else {
                        0
                    }
                }
            }
        }
        Some(FileDescriptor::PtySlave { ref pair, .. }) => {
            let mut data = alloc::vec![0u8; count];
            let ldisc = crate::pty::PtyLineDiscipline::default();
            match crate::pty::pty_read_slave(pair, &mut data, &ldisc) {
                Ok(n) => {
                    if n == 0 {
                        return 0;
                    }
                    if unsafe { user_access::copy_to_user(buf, &data[..n]) }.is_err() {
                        return errno::Errno::EFAULT as u64;
                    }
                    n as u64
                }
                Err(()) => {
                    if count > 0 {
                        errno::Errno::EAGAIN as u64
                    } else {
                        0
                    }
                }
            }
        }
        Some(FileDescriptor::Socket(handle, stype)) => {
            let mut recv_buf = alloc::vec![0u8; count];
            let mut sockets = crate::net::SOCKETS.lock();
            match super::net::recvfrom_internal(&mut sockets, handle, stype, &mut recv_buf) {
                Ok((n, _ep)) => {
                    if n == 0 {
                        return 0;
                    }
                    if unsafe { user_access::copy_to_user(buf, &recv_buf[..n]) }.is_err() {
                        return errno::Errno::EFAULT as u64;
                    }
                    n as u64
                }
                Err(e) => e,
            }
        }
        Some(FileDescriptor::UnixSocket(handle, _)) => {
            let mut recv_buf = alloc::vec![0u8; count];
            match crate::net::unix::recvfrom_unix(
                handle,
                recv_buf.as_mut_ptr() as *mut u8,
                count as u64,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            ) {
                Ok(n) => {
                    if unsafe { user_access::copy_to_user(buf, &recv_buf[..n as usize]) }.is_err() {
                        return errno::Errno::EFAULT as u64;
                    }
                    n
                }
                Err(e) => e as u64,
            }
        }
        Some(FileDescriptor::SignalFd(handle)) => {
            let sig_fds = super::SIGNAL_FDS.lock();
            if let Some(data) = sig_fds.get(&handle) {
                let mut d = data.lock();
                if let Some(info) = d.pending.pop_front() {
                    // Write the full 128-byte signalfd_siginfo struct to userspace
                    let bytes = unsafe {
                        core::slice::from_raw_parts(
                            &info as *const _ as *const u8,
                            core::mem::size_of::<crate::task::process::SignalFdInfo>(),
                        )
                    };
                    if unsafe { user_access::copy_to_user(buf, bytes) }.is_err() {
                        return errno::Errno::EFAULT as u64;
                    }
                    core::mem::size_of::<crate::task::process::SignalFdInfo>() as u64
                } else {
                    errno::Errno::EAGAIN as u64
                }
            } else {
                errno::Errno::EBADF as u64
            }
        }
        Some(FileDescriptor::EventFd(ref data_arc)) => {
            let d_arc = data_arc.clone();

            // Loop: block until counter > 0 (or nonblock → EAGAIN)
            loop {
                let (key, nonblock) = {
                    let mut d = d_arc.lock();
                    if d.counter > 0 {
                        let val = if d.semaphore {
                            d.counter -= 1;
                            1u64
                        } else {
                            let v = d.counter;
                            d.counter = 0;
                            v
                        };
                        drop(d);
                        let bytes = val.to_ne_bytes();
                        if unsafe { user_access::copy_to_user(buf, &bytes) }.is_err() {
                            return errno::Errno::EFAULT as u64;
                        }
                        return 8;
                    }
                    let key = d.key;
                    let nb = d.nonblock;
                    drop(d);
                    (key, nb)
                };
                if nonblock {
                    return errno::Errno::EAGAIN as u64;
                }
                // Check for pending signals before blocking
                if crate::syscalls::signal::has_pending_signal() {
                    return errno::Errno::EINTR as u64;
                }
                crate::task::scheduler::block_on_pipe(key);
            }
        }
        Some(FileDescriptor::TimerFd(ref tfd_arc)) => {
            let d_arc = tfd_arc.clone();

            // Loop: block until expirations > 0 (or nonblock → EAGAIN)
            loop {
                let (key, nonblock) = {
                    let mut d = d_arc.lock();
                    if d.expirations > 0 {
                        let exp = d.expirations;
                        d.expirations = 0;
                        drop(d);
                        let bytes = exp.to_ne_bytes();
                        if unsafe { user_access::copy_to_user(buf, &bytes) }.is_err() {
                            return errno::Errno::EFAULT as u64;
                        }
                        return 8;
                    }
                    let key = d.key;
                    let nb = d.nonblock;
                    drop(d);
                    (key, nb)
                };
                if nonblock {
                    return errno::Errno::EAGAIN as u64;
                }
                // Check for pending signals before blocking
                if crate::syscalls::signal::has_pending_signal() {
                    return errno::Errno::EINTR as u64;
                }
                crate::task::scheduler::block_on_pipe(key);
            }
        }
        Some(FileDescriptor::InotifyFd { instance_key, .. }) => {
            let key = instance_key;

            // inotify_read fills a kernel buffer; the user pointer is only
            // touched by the fault-aware copy below, never as a raw slice.
            // ponytail: cap scratch space at 4 KiB; larger reads need chunked draining.
            let mut kbuf = alloc::vec![0u8; count.min(4096)];
            match super::inotify::inotify_read(key, &mut kbuf) {
                Ok(n) => {
                    if unsafe { user_access::copy_to_user(buf, &kbuf[..n]) }.is_err() {
                        return errno::Errno::EFAULT as u64;
                    }
                    n as u64
                }
                Err(e) => e as u64,
            }
        }
        Some(FileDescriptor::IoUringFd(ref inst_arc)) => {
            let mut guard = inst_arc.lock();
            let inst = guard
                .downcast_mut::<crate::syscalls::io_uring::IoUringInstance>()
                .expect("IoUringFd did not contain IoUringInstance");
            if inst.peek_cqes() == 0 {
                return errno::Errno::EAGAIN as u64;
            }
            let max_cqes = count / core::mem::size_of::<crate::syscalls::io_uring::IoCqe>();
            let mut cqe_buf = alloc::vec![crate::syscalls::io_uring::IoCqe::default(); max_cqes];
            let n = inst.drain_cqes(&mut cqe_buf);
            if n == 0 {
                return errno::Errno::EAGAIN as u64;
            }
            let bytes = n * core::mem::size_of::<crate::syscalls::io_uring::IoCqe>();
            let src = unsafe { core::slice::from_raw_parts(cqe_buf.as_ptr() as *const u8, bytes) };
            if unsafe { user_access::copy_to_user(buf, src) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            bytes as u64
        }
        // Linux exposes `read(2)` on a queue descriptor (and its /dev/mqueue
        // file): the status line, with the file position per description.
        Some(FileDescriptor::Mqueue(ref obj)) => {
            if !obj.can_read() {
                return errno::Errno::EBADF as u64;
            }
            let bytes = obj.read_line(count);
            if bytes.is_empty() {
                return 0;
            }
            // SAFETY: `buf` is the caller's userspace buffer; `copy_to_user`
            // validates the range and turns a fault into `Err`.
            if unsafe { user_access::copy_to_user(buf, &bytes) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            bytes.len() as u64
        }
        None => errno::Errno::EBADF as u64,
    }
}

// ─── write ────────────────────────────────────────────────────────

pub fn sys_write(fd: u64, buf: *const u8, count: usize) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let mut data = alloc::vec![0u8; count];
    if unsafe { user_access::copy_from_user(&mut data, buf) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    let fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }
    match fd_table[fd as usize] {
        Some(FileDescriptor::File {
            ref node,
            ref offset,
        }) => {
            let mut off = offset.lock();
            let pos = *off;
            match node.write(&data) {
                Ok(()) => {
                    *off = pos + count;
                    // Fire inotify IN_MODIFY event for the file
                    if let Ok(_st) = node.stat() {
                        let path = alloc::format!("/fd/{}", fd);
                        super::inotify::inotify_emit(&path, super::inotify::IN_MODIFY, 0, "");
                    }
                    count as u64
                }
                Err(_) => errno::Errno::EIO as u64,
            }
        }
        Some(FileDescriptor::PtyMaster { ref pair, .. }) => {
            match crate::pty::pty_write_master(pair, &data) {
                Ok(n) => n as u64,
                Err(()) => errno::Errno::EAGAIN as u64,
            }
        }
        Some(FileDescriptor::PtySlave { ref pair, .. }) => {
            match crate::pty::pty_write_slave(pair, &data) {
                Ok(n) => n as u64,
                Err(()) => errno::Errno::EAGAIN as u64,
            }
        }
        Some(FileDescriptor::Socket(handle, stype)) => {
            let mut sockets = crate::net::SOCKETS.lock();
            super::net::sendto_internal(&mut sockets, handle, stype, &data, None)
        }
        Some(FileDescriptor::UnixSocket(handle, _)) => {
            match crate::net::unix::sendto_unix(
                handle,
                data.as_ptr() as *const u8,
                count as u64,
                core::ptr::null(),
                0,
            ) {
                Ok(n) => n,
                Err(e) => e as u64,
            }
        }
        Some(FileDescriptor::EventFd(ref data_arc)) => {
            let d_arc = data_arc.clone();

            let mut d = d_arc.lock();
            let val = u64::from_ne_bytes(data[..8].try_into().unwrap_or([0; 8]));
            // eventfd treats UINT64_MAX as an invalid write (EINVAL per man page)
            if val == u64::MAX {
                return errno::Errno::EINVAL as u64;
            }
            // Overflow check: counter + val must not exceed EFD_MAX (0xFFFF_FFFF_FFFF_FFFE)
            if val > crate::task::process::EFD_MAX
                || d.counter > crate::task::process::EFD_MAX - val
            {
                return errno::Errno::EFBIG as u64;
            }
            d.counter += val;
            let key = d.key;
            drop(d);
            // Wake any threads blocked reading from this eventfd
            crate::task::scheduler::wake_pipe(key);
            8
        }
        Some(FileDescriptor::IoUringFd(ref inst_arc)) => {
            // write() on io_uring fd submits SQEs (alternative to io_uring_enter)
            let sqe_size = core::mem::size_of::<crate::syscalls::io_uring::IoSqEntry>();
            let n_sqes = count / sqe_size;
            if n_sqes == 0 {
                return errno::Errno::EINVAL as u64;
            }
            let mut guard = inst_arc.lock();
            let inst = guard
                .downcast_mut::<crate::syscalls::io_uring::IoUringInstance>()
                .expect("IoUringFd did not contain IoUringInstance");
            // Decode the SQEs from the kernel copy made at the top of
            // sys_write: submit_sqes reads every entry, so the user buffer
            // must never be reinterpreted as a typed slice.
            let mut sqes = alloc::vec![crate::syscalls::io_uring::IoSqEntry::default(); n_sqes];
            // SAFETY: both sides are byte buffers of the same length; `data`
            // was filled by copy_from_user and IoSqEntry is plain data.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    sqes.as_mut_ptr() as *mut u8,
                    n_sqes * sqe_size,
                );
            }
            let submitted = inst.submit_sqes(&sqes);
            if submitted > 0 {
                inst.process_all();
            }
            (submitted * sqe_size) as u64
        }
        _ => errno::Errno::EINVAL as u64,
    }
}

// ─── lseek ────────────────────────────────────────────────────────

pub fn sys_lseek(fd: u64, offset: i64, whence: i32) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }
    match fd_table[fd as usize] {
        Some(FileDescriptor::File {
            ref node,
            offset: ref file_off,
        }) => {
            let stat = match node.stat() {
                Ok(s) => s,
                Err(_) => return errno::Errno::EIO as u64,
            };
            let cur = *file_off.lock();
            let new_pos = match whence {
                0 => offset as usize,
                1 => (cur as i64 + offset) as usize,
                2 => (stat.st_size as i64 + offset) as usize,
                _ => return errno::Errno::EINVAL as u64,
            };
            *file_off.lock() = new_pos;
            new_pos as u64
        }
        _ => errno::Errno::ESPIPE as u64,
    }
}

// ─── brk ──────────────────────────────────────────────────────────

pub fn sys_brk(addr: u64) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let current_brk = process.memory.lock().brk;
    if addr == 0 {
        return current_brk;
    }
    if addr > current_brk {
        process.add_vma(crate::task::process::Vma {
            start: current_brk,
            end: addr,
            flags: PageTableFlags::PRESENT
                | PageTableFlags::USER_ACCESSIBLE
                | PageTableFlags::WRITABLE,
            _name: "brk",
            file_handle: None,
            file_offset: 0,
            is_shared: false,
            shm_id: None,
        });
    }
    process.memory.lock().brk = addr;
    addr
}

// ─── mmap / munmap / mprotect ─────────────────────────────────────

pub fn sys_mmap(addr: u64, len: u64, prot: u64, flags: u64, fd: u64, offset: u64) -> u64 {
    let page_size: u64 = 4096;
    let len = (len + page_size - 1) & !(page_size - 1);
    if len == 0 {
        return errno::Errno::EINVAL as u64;
    }
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
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
    // Anonymous mmap (addr == 0): pick the next VMA-free region at or above a
    // brk-aligned hint. An explicit addr is treated as a hint too (Linux
    // non-MAP_FIXED semantics): if its region is occupied we scan forward,
    // never alias it. This used to return the same fixed address for every
    // call, so distinct allocations aliased one page and userspace heap data
    // silently overwrote itself (fixed: find_free_vma_region scans forward).
    // The find + insert MUST happen under ONE memory-lock guard: scanning
    // under the lock and then re-locking in add_vma let two concurrent mmaps
    // (multithreaded malloc arenas) both pick the same free region and alias
    // each other, which surfaced as userspace reads 8B-past a chunk the
    // second allocator believed it owned.
    let hint = {
        let mem = process.memory.lock();
        if addr == 0 {
            let align = 0x1000_0000u64;
            (mem.brk + align - 1) & !(align - 1)
        } else {
            addr
        }
    };
    let max_addr: u64 = 0x0000_7000_0000_0000; // stay clear of the user stack at ~0x7fff...
    let mut page_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if prot & 2 != 0 {
        page_flags |= PageTableFlags::WRITABLE;
    }
    if prot & 4 == 0 {
        page_flags |= PageTableFlags::NO_EXECUTE;
    }
    let is_shared = flags & 0x02 != 0;
    let alloc_addr;
    {
        let mut mem = process.memory.lock();
        match crate::task::process::find_free_vma_region(&mem.vmas, hint, len, max_addr) {
            Some(a) => alloc_addr = a,
            None => return errno::Errno::ENOMEM as u64,
        }
        let vma = crate::task::process::Vma {
            start: alloc_addr,
            end: alloc_addr + len,
            flags: page_flags,
            _name: "mmap",
            file_handle: if fd != u64::MAX { Some(fd) } else { None },
            file_offset: offset,
            is_shared,
            shm_id: None,
        };
        // Isolate mmap delegation is deferred to a future iteration; the
        // legacy VMA path below handles the mapping correctly.
        crate::task::process::Process::insert_vma_locked(&mut mem.vmas, vma);
    }
    alloc_addr
}

pub fn sys_munmap(addr: u64, len: u64) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    // Isolate munmap delegation is deferred to a future iteration.
    process.remove_vma_range(addr, addr + len);
    0
}

pub fn sys_mprotect(addr: u64, len: u64, prot: u64) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let page_size: u64 = 4096;
    let aligned_end = (addr + len + page_size - 1) & !(page_size - 1);
    let aligned_addr = addr & !(page_size - 1);
    let mut new_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if prot & 2 != 0 {
        new_flags |= PageTableFlags::WRITABLE;
    }
    if prot & 4 == 0 {
        new_flags |= PageTableFlags::NO_EXECUTE;
    }
    // Isolate mprotect delegation is deferred to a future iteration.
    // Update legacy VMA flags (must write back, not just modify a clone)
    {
        let mut mem = process.memory.lock();
        for vma in mem.vmas.iter_mut() {
            if vma.start < aligned_end && vma.end > aligned_addr {
                vma.flags = new_flags;
            }
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
        if let Some(Some(FileDescriptor::File { node, .. })) = fd_table.get(fd as usize) {
            node.clone()
        } else {
            return errno::Errno::EBADF as u64;
        }
    } else {
        return errno::Errno::EBADF as u64;
    };
    if !node.is_dir() {
        return errno::Errno::ENOTDIR as u64;
    }
    drop(proc);
    let children = match node.children() {
        Ok(c) => c,
        Err(_) => return errno::Errno::EIO as u64,
    };
    let mut written: usize = 0;
    for child in &children {
        let name = child.name();
        let name_bytes = name.as_bytes();
        let reclen = ((core::mem::size_of::<u64>() * 3)
            .saturating_add(name_bytes.len())
            .saturating_add(1 + 7))
            & !7;
        if written + reclen > len {
            break;
        }
        let entry_offset = written;
        let d_type = if child.is_dir() { 4u8 } else { 8u8 };
        let dirent = LinuxDirent64 {
            d_ino: 1,
            d_off: (written + reclen) as u64,
            d_reclen: reclen as u16,
            d_type,
        };
        let dirent_bytes = unsafe {
            core::slice::from_raw_parts(
                &dirent as *const _ as *const u8,
                core::mem::size_of::<LinuxDirent64>(),
            )
        };
        if unsafe { buf.add(entry_offset) }.is_null() {
            return errno::Errno::EFAULT as u64;
        }
        unsafe {
            if user_access::copy_to_user(buf.add(entry_offset), dirent_bytes).is_err() {
                return errno::Errno::EFAULT as u64;
            }
        }
        let name_offset = entry_offset + core::mem::size_of::<LinuxDirent64>();
        unsafe {
            if user_access::copy_to_user(buf.add(name_offset), name_bytes).is_err() {
                return errno::Errno::EFAULT as u64;
            }
        }
        if name_offset + name_bytes.len() < entry_offset + reclen {
            let null_byte = [0u8];
            unsafe {
                if user_access::copy_to_user(buf.add(name_offset + name_bytes.len()), &null_byte)
                    .is_err()
                {
                    return errno::Errno::EFAULT as u64;
                }
            }
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
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }
    match fd_table[fd as usize] {
        Some(FileDescriptor::PtyMaster { .. }) | Some(FileDescriptor::PtySlave { .. }) => {
            match request {
                TIOCGWINSZ => {
                    let ws = Winsize {
                        ws_row: 24,
                        ws_col: 80,
                        ws_xpixel: 640,
                        ws_ypixel: 400,
                    };
                    if unsafe {
                        user_access::copy_to_user(
                            argp,
                            core::slice::from_raw_parts(
                                &ws as *const _ as *const u8,
                                core::mem::size_of::<Winsize>(),
                            ),
                        )
                    }
                    .is_err()
                    {
                        return errno::Errno::EFAULT as u64;
                    }
                    0
                }
                TCGETS => {
                    let t = Termios {
                        c_iflag: 0,
                        c_oflag: 0,
                        c_cflag: 0x800 | 0x32,
                        c_lflag: 0x8a3b,
                        c_cc: [0; 19],
                    };
                    if unsafe {
                        user_access::copy_to_user(
                            argp,
                            core::slice::from_raw_parts(
                                &t as *const _ as *const u8,
                                core::mem::size_of::<Termios>(),
                            ),
                        )
                    }
                    .is_err()
                    {
                        return errno::Errno::EFAULT as u64;
                    }
                    0
                }
                TCSETS => 0,
                FIONBIO => 0,
                _ => errno::Errno::ENOTTY as u64,
            }
        }
        Some(FileDescriptor::File { ref node, .. }) => match request {
            TIOCGWINSZ => {
                let ws = Winsize {
                    ws_row: 24,
                    ws_col: 80,
                    ws_xpixel: 640,
                    ws_ypixel: 400,
                };
                if unsafe {
                    user_access::copy_to_user(
                        argp,
                        core::slice::from_raw_parts(
                            &ws as *const _ as *const u8,
                            core::mem::size_of::<Winsize>(),
                        ),
                    )
                }
                .is_err()
                {
                    return errno::Errno::EFAULT as u64;
                }
                0
            }
            _ => match node.ioctl(request, argp) {
                Ok(v) => v,
                Err(()) => errno::Errno::ENOTTY as u64,
            },
        },
        Some(FileDescriptor::Socket(_handle, _stype)) => match request {
            0x5421 => 0,
            _ => errno::Errno::ENOTTY as u64,
        },
        _ => errno::Errno::ENOTTY as u64,
    }
}

// ─── fallocate / sendfile ─────────────────────────────────────────

pub fn sys_fallocate(fd: u64, mode: i32, offset: i64, len: i64) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }
    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, .. }) => match node.fallocate(mode, offset, len) {
            Ok(()) => 0,
            Err(_) => errno::Errno::ENOSPC as u64,
        },
        _ => errno::Errno::EBADF as u64,
    }
}

pub fn sys_sendfile(out_fd: u64, in_fd: u64, offset_ptr: *mut u64, count: u64) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let fd_table = process.files.lock().fd_table.clone();
    if (out_fd as usize) >= fd_table.len() || (in_fd as usize) >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }
    let start_offset = if !offset_ptr.is_null() {
        let slice = unsafe { core::slice::from_raw_parts(offset_ptr as *const u8, 8) };
        u64::from_ne_bytes(slice.try_into().unwrap_or([0; 8]))
    } else {
        0
    };
    let buf_size = core::cmp::min(count, 4096) as usize;
    let mut buf = alloc::vec![0u8; buf_size];
    match fd_table[in_fd as usize] {
        Some(FileDescriptor::File {
            ref node,
            offset: ref in_off,
            ..
        }) => {
            let mut read_pos = if !offset_ptr.is_null() {
                start_offset
            } else {
                *in_off.lock() as u64
            };
            let mut total: u64 = 0;
            while total < count {
                let n = match node.read(buf.len()) {
                    Ok(bytes) => {
                        let l = bytes.len();
                        buf[..l].copy_from_slice(&bytes);
                        l
                    }
                    Err(_) => break,
                };
                if n == 0 {
                    break;
                }
                match fd_table[out_fd as usize] {
                    Some(FileDescriptor::File { ref node, .. }) => {
                        let _ = node.write(&buf[..n]);
                    }
                    _ => return errno::Errno::EBADF as u64,
                }
                if !offset_ptr.is_null() {
                    read_pos += n as u64;
                } else {
                    *in_off.lock() += n as usize;
                }
                total += n as u64;
            }
            if !offset_ptr.is_null() {
                let new_offset = read_pos.to_ne_bytes();
                if unsafe { user_access::copy_to_user(offset_ptr as *mut u8, &new_offset) }.is_err()
                {
                    return errno::Errno::EFAULT as u64;
                }
            }
            total as u64
        }
        _ => errno::Errno::EBADF as u64,
    }
}

// ─── fsync / fdatasync ──────────────────────────────────────────────

pub fn sys_fsync(_fd: u64) -> u64 {
    errno::Errno::ENOSYS as u64
}

pub fn sys_fdatasync(_fd: u64) -> u64 {
    errno::Errno::ENOSYS as u64
}

// ─── getdents ────────────────────────────────────────────────────────

pub fn sys_getdents(_fd: u64, _buf: *mut u8, _count: usize) -> u64 {
    errno::Errno::ENOSYS as u64
}

// ─── msync ─────────────────────────────────────────────────────────────
/// msync(addr, len, flags) → 0 on success
///
/// Synchronizes a mapped region to backing storage.
/// flags: MS_ASYNC (1), MS_SYNC (2), MS_INVALIDATE (4)
pub fn sys_msync(addr: u64, len: usize, flags: i32) -> u64 {
    let page_size: u64 = 4096;
    // Linux has no zero-length check: an empty range is simply a no-op.
    if len == 0 {
        return 0;
    }
    if addr & (page_size - 1) != 0 {
        return errno::Errno::EINVAL as u64;
    }
    // MS_ASYNC and MS_SYNC together are contradictory; Linux rejects the pair.
    let contradictory = (flags & MS_ASYNC) != 0 && (flags & MS_SYNC) != 0;
    if (flags & !(MS_ASYNC | MS_SYNC | MS_INVALIDATE)) != 0 || contradictory {
        return errno::Errno::EINVAL as u64;
    }

    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    let end = addr + len as u64;
    let mem = process.memory.lock();
    let mut page = addr;
    while page < end {
        let vma = match mem.vmas.iter().find(|v| page >= v.start && page < v.end) {
            Some(v) => v,
            // Linux msync: a hole in the range is ENOMEM.
            None => return errno::Errno::ENOMEM as u64,
        };
        if vma.is_shared && vma.file_handle.is_some() {
            // ponytail: no page cache and no dirty-page write-back exist — sys_mmap
            // never loads file contents, so there is nothing to flush. Upgrade when
            // file-backed mmap gains a page cache with dirty tracking.
            return errno::Errno::EOPNOTSUPP as u64;
        }
        // Anonymous/private pages are already in the page tables; MS_ASYNC,
        // MS_SYNC and MS_INVALIDATE are satisfied by skipping to the VMA end.
        page = vma.end;
    }

    0
}

// ─── mremap ────────────────────────────────────────────────────────────
/// mremap(old_address, old_size, new_size, flags, new_address) → new_address
///
/// Expands, shrinks, or moves a virtual memory region.
/// flags: MREMAP_MAYMOVE (1), MREMAP_FIXED (2)
pub fn sys_mremap(
    old_address: u64,
    old_size: usize,
    new_size: usize,
    flags: i32,
    new_address: u64,
) -> u64 {
    let page_size: u64 = 4096;
    let max_addr: u64 = 0x0000_7000_0000_0000;

    if old_address & (page_size - 1) != 0 {
        return errno::Errno::EINVAL as u64;
    }
    let page_mask = (page_size - 1) as usize;
    if old_size == 0 || old_size & page_mask != 0 {
        return errno::Errno::EINVAL as u64;
    }
    if new_size == 0 || new_size & page_mask != 0 {
        return errno::Errno::EINVAL as u64;
    }
    if (flags & !(MREMAP_MAYMOVE | MREMAP_FIXED)) != 0 {
        return errno::Errno::EINVAL as u64;
    }
    let may_move = (flags & MREMAP_MAYMOVE) != 0;
    let fixed = (flags & MREMAP_FIXED) != 0;
    if fixed && !may_move {
        return errno::Errno::EINVAL as u64;
    }

    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    // shortcut: only an exact-start, whole-VMA remap is supported; a partial
    // range would need VMA splitting. Upgrade when a caller needs it.
    let vma = {
        let mem = process.memory.lock();
        match mem.vmas.iter().find(|v| v.start == old_address) {
            Some(v) if v.end - v.start == old_size as u64 => v.clone(),
            Some(_) => return errno::Errno::EINVAL as u64,
            None => return errno::Errno::EFAULT as u64,
        }
    };
    let old_end = old_address + old_size as u64;
    let new_end = old_address + new_size as u64;

    if new_size == old_size && !fixed {
        return old_address;
    }

    if new_size < old_size {
        // SAFETY: the syscall runs on the caller's own address space, so its
        // PML4 is the active one and mapper() may mutate it.
        if let Some(mut mapper) = unsafe { process.address_space.mapper() } {
            let mut page_addr = new_end;
            while page_addr < old_end {
                let page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(page_addr));
                if let Ok((frame, flush)) = mapper.unmap(page) {
                    flush.flush();
                    // Refcounted release: a COW-shared frame stays alive for the
                    // other address space, and hits 0 -> deferred free otherwise.
                    crate::memory::frame_info::decrement(frame.start_address());
                }
                page_addr += page_size;
            }
        }
        let mut mem = process.memory.lock();
        if let Some(v) = mem.vmas.iter_mut().find(|v| v.start == old_address) {
            v.end = new_end;
        }
        return old_address;
    }

    if new_size > old_size {
        let mut mem = process.memory.lock();
        let mut blocked = new_end > max_addr;
        for v in mem.vmas.iter() {
            if v.start < new_end && v.end > old_end {
                blocked = true;
            }
        }
        if !blocked {
            if let Some(v) = mem.vmas.iter_mut().find(|v| v.start == old_address) {
                v.end = new_end;
            }
            // Pages past old_end stay unmapped; the page fault path maps them
            // from the VMA on first touch.
            return old_address;
        }
        if !may_move {
            return errno::Errno::EAGAIN as u64;
        }
    }

    let dest = {
        let mem = process.memory.lock();
        if fixed {
            if new_address & (page_size - 1) != 0 {
                return errno::Errno::EINVAL as u64;
            }
            let dest_end = new_address + new_size as u64;
            // The destination must be free, source VMA included: a partially
            // overlapping move has no defined outcome.
            let mut taken = false;
            for v in mem.vmas.iter() {
                if v.start < dest_end && v.end > new_address {
                    taken = true;
                    break;
                }
            }
            if taken {
                return errno::Errno::EINVAL as u64;
            }
            new_address
        } else {
            let hint = if new_address != 0 {
                new_address
            } else {
                old_end
            };
            match crate::task::process::find_free_vma_region(
                &mem.vmas,
                hint,
                new_size as u64,
                max_addr,
            ) {
                Some(addr) => addr,
                None => return errno::Errno::ENOMEM as u64,
            }
        }
    };

    let flags_with_present = vma.flags | PageTableFlags::PRESENT;
    // SAFETY: the syscall runs on the caller's own address space, so its PML4
    // is the active one and mapper() may mutate it.
    let mut mapper = match unsafe { process.address_space.mapper() } {
        Some(m) => m,
        None => return errno::Errno::ENOMEM as u64,
    };
    let mut frame_allocator = crate::memory::buddy::BuddyFrameAllocator;
    let mut page_addr = old_address;
    while page_addr < old_end {
        let src = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(page_addr));
        let dst = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(
            dest + (page_addr - old_address),
        ));
        if let Ok((frame, flush)) = mapper.unmap(src) {
            flush.flush();
            // SAFETY: map_to installs a fresh leaf entry for dst pointing at the
            // frame just unmapped from src; page-table frames come from the
            // buddy allocator.
            match unsafe { mapper.map_to(dst, frame, flags_with_present, &mut frame_allocator) } {
                Ok(flush) => flush.flush(),
                Err(_) => {
                    // SAFETY: restoring the frame that was just unmapped from src
                    // into the entry it came from; src is inside the source VMA.
                    match unsafe {
                        mapper.map_to(src, frame, flags_with_present, &mut frame_allocator)
                    } {
                        Ok(flush) => flush.flush(),
                        // Second failure: the page is lost, so release the frame.
                        Err(_) => {
                            crate::memory::frame_info::decrement(frame.start_address());
                        }
                    }
                    // Roll back the pages moved so far, then report the failure.
                    let mut done = old_address;
                    while done < page_addr {
                        let s = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(done));
                        let d = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(
                            dest + (done - old_address),
                        ));
                        if let Ok((moved, flush)) = mapper.unmap(d) {
                            flush.flush();
                            // SAFETY: restoring the mapping that was moved away.
                            if let Ok(flush) = unsafe {
                                mapper.map_to(s, moved, flags_with_present, &mut frame_allocator)
                            } {
                                flush.flush();
                            }
                        }
                        done += page_size;
                    }
                    return errno::Errno::ENOMEM as u64;
                }
            }
        }
        page_addr += page_size;
    }

    {
        let mut mem = process.memory.lock();
        mem.vmas.retain(|v| v.start != old_address);
        crate::task::process::Process::insert_vma_locked(
            &mut mem.vmas,
            crate::task::process::Vma {
                start: dest,
                end: dest + new_size as u64,
                flags: vma.flags,
                _name: vma._name,
                file_handle: vma.file_handle,
                file_offset: vma.file_offset,
                is_shared: vma.is_shared,
                shm_id: vma.shm_id,
            },
        );
    }

    dest
}

pub fn sys_mincore(addr: u64, len: usize, vec: *mut u8) -> u64 {
    let page_size: u64 = 4096;

    // Validate address range and alignment
    // Linux has no zero-length check: pages = 0, so nothing is reported.
    if len == 0 {
        return 0;
    }
    if addr & (page_size - 1) != 0 {
        return errno::Errno::EINVAL as u64;
    }
    if vec.is_null() {
        return errno::Errno::EFAULT as u64;
    }

    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    let end = addr + len as u64;
    let page_count = (len + page_size as usize - 1) / page_size as usize;
    // Linux mincore reports ONE BYTE PER PAGE, not a bit vector: a bit vector
    // makes glibc/musl read residency flags out of the wrong byte.
    let mut result = alloc::vec![0u8; page_count];

    // A hole inside the range is ENOMEM, checked before touching the tables.
    {
        let mem = process.memory.lock();
        let mut page = addr;
        while page < end {
            match mem.vmas.iter().find(|v| page >= v.start && page < v.end) {
                Some(v) => page = v.end,
                None => return errno::Errno::ENOMEM as u64,
            }
        }
    }

    // SAFETY: the syscall runs on the caller's own address space, so its PML4
    // is the active one; translate_page only reads it.
    if let Some(mapper) = unsafe { process.address_space.mapper() } {
        for (i, slot) in result.iter_mut().enumerate() {
            let page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(
                addr + i as u64 * page_size,
            ));
            if mapper.translate_page(page).is_ok() {
                *slot = 1;
            }
        }
    }

    // SAFETY: `vec` is non-null and copy_to_user validates the userspace range.
    unsafe {
        if crate::syscalls::user_access::copy_to_user(vec, &result).is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }

    0
}

// ── vfork ───────────────────────────────────────────────────────────

/// vfork() → child PID in the parent, 0 in the child.
///
/// This is a plain fork: the child gets its own COW address space and the
/// parent keeps running.
/// ponytail: no address-space sharing and no parent suspension; upgrade when a
/// caller depends on vfork's no-copy/blocked-parent guarantee — the usual
/// immediate exec-then-wait idiom is already correct under a plain fork.
pub fn sys_vfork() -> u64 {
    sys_fork(core::ptr::null_mut())
}

// ── shutdown ────────────────────────────────────────────────────────

pub fn sys_shutdown(_fd: u64, _how: i32) -> u64 {
    errno::Errno::ENOSYS as u64
}
