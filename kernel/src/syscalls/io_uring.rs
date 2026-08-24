//! io_uring — asynchronous I/O interface for the Vahi kernel.
//!
//! SQ ring: userspace writes SQEs at sq.tail, kernel reads at sq.head.
//! CQ ring: kernel writes CQEs at cq.tail, userspace reads at cq.head.
//! eventfd integration for async completion notification.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::IrqSafeMutex as Mutex;
use crate::syscalls::errno::Errno;
use crate::syscalls::user_access;
use crate::task::process::{CURRENT_PROCESS, FileDescriptor};

// ── Constants ──────────────────────────────────────────────────────

pub const IORING_ENTER_GETEVENTS: u32 = 1;

pub const IORING_REGISTER_BUFFERS: u32 = 0;
pub const IORING_UNREGISTER_BUFFERS: u32 = 1;
pub const IORING_REGISTER_FILES: u32 = 2;
pub const IORING_UNREGISTER_FILES: u32 = 3;
pub const IORING_REGISTER_EVENTFD: u32 = 4;
pub const IORING_UNREGISTER_EVENTFD: u32 = 5;

pub const IORING_OP_NOP: u8 = 0;
pub const IORING_OP_READV: u8 = 1;
pub const IORING_OP_WRITEV: u8 = 2;
pub const IORING_OP_CLOSE: u8 = 5;
pub const IORING_OP_POLL_ADD: u8 = 6;
pub const IORING_OP_POLL_REMOVE: u8 = 7;

pub const IOSQE_IO_LINK: u8 = 0x02;

pub const IORING_MAX_ENTRIES: u32 = 4096;

// ── Data structures ────────────────────────────────────────────────

/// 64-byte submission queue entry. Matches Linux io_uring_sqe layout.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoSqEntry {
    pub opcode: u8,
    pub flags: u8,
    pub ioprio: u16,
    pub fd: i32,
    pub off: u64,
    pub addr: u64,
    pub len: u32,
    pub buf_group: u16,
    pub user_data: u64,
    pub buf_index: u16,
    pub personality: u16,
    pub splice_fd_in: i32,
    _reserved: [u8; 12],
}

/// 16-byte completion queue entry. Matches Linux io_uring_cqe layout.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoCqe {
    pub user_data: u64,
    pub res: i32,
    pub flags: u32,
}

/// Ring buffer metadata shared between kernel and userspace.
#[repr(C)]
#[derive(Default)]
pub struct IoRingRing {
    pub head: u32,
    pub tail: u32,
    pub ring_mask: u32,
    pub ring_entries: u32,
}

pub struct IoUringInstance {
    pub sq_ring: IoRingRing,
    pub sq_entries: Vec<IoSqEntry>,
    pub cq_ring: IoRingRing,
    pub cq_entries: Vec<IoCqe>,
    pub fixed_buffers: Vec<(u64, u32)>,
    pub fixed_files: Vec<i32>,
    pub eventfd: Option<Arc<Mutex<crate::task::process::EventFdData>>>,
    pub key: u64,
}

impl IoUringInstance {
    pub fn new(entries: u32) -> Self {
        let n = entries as usize;
        let ring_mask = (entries - 1) as u32;
        let cq_entries = entries * 2;
        IoUringInstance {
            sq_ring: IoRingRing { head: 0, tail: 0, ring_mask, ring_entries: entries },
            sq_entries: Vec::with_capacity(n),
            cq_ring: IoRingRing { head: 0, tail: 0, ring_mask: (cq_entries - 1), ring_entries: cq_entries },
            cq_entries: Vec::with_capacity(cq_entries as usize),
            fixed_buffers: Vec::new(),
            fixed_files: Vec::new(),
            eventfd: None,
            key: next_io_uring_key(),
        }
    }

    pub fn submit_sqes(&mut self, sqes: &[IoSqEntry]) -> usize {
        let mask = self.sq_ring.ring_mask as usize;
        let ring_cap = self.sq_ring.ring_entries as usize;
        let tail = self.sq_ring.tail as usize;
        let mut accepted = 0usize;
        for sqe in sqes {
            if accepted >= ring_cap { break; }
            let idx = (tail + accepted) & mask;
            while self.sq_entries.len() <= idx {
                self.sq_entries.push(IoSqEntry::default());
            }
            self.sq_entries[idx] = *sqe;
            accepted += 1;
        }
        self.sq_ring.tail = (tail + accepted) as u32;
        accepted
    }

    pub fn process_all(&mut self) -> usize {
        let mask = self.sq_ring.ring_mask as usize;
        let head = self.sq_ring.head as usize;
        let tail = self.sq_ring.tail as usize;
        let mut processed = 0usize;
        let mut link_chain_failed = false;
        let mut pos = head;
        while pos != tail {
            let idx = pos & mask;
            let sqe = match self.sq_entries.get(idx) {
                Some(s) => *s,
                None => break,
            };
            let is_linked = sqe.flags & IOSQE_IO_LINK != 0;
            if is_linked && link_chain_failed {
                self.push_cqe(IoCqe { user_data: sqe.user_data, res: Errno::ECANCELED as i32, flags: 0 });
                pos += 1;
                continue;
            }
            let cqe = process_sqe(&sqe);
            let failed = cqe.res < 0;
            self.push_cqe(cqe);
            processed += 1;
            pos += 1;
            if !is_linked { link_chain_failed = false; } else if failed { link_chain_failed = true; }
        }
        self.sq_ring.head = (head + processed) as u32;
        self.sq_entries.clear();
        if processed > 0 { self.notify_eventfd(); }
        processed
    }

    fn push_cqe(&mut self, cqe: IoCqe) {
        let idx = self.cq_ring.tail as usize & self.cq_ring.ring_mask as usize;
        if self.cq_entries.len() <= idx {
            self.cq_entries.resize(idx + 1, IoCqe::default());
        }
        self.cq_entries[idx] = cqe;
        self.cq_ring.tail = self.cq_ring.tail.wrapping_add(1);
    }

    fn notify_eventfd(&self) {
        if let Some(ref efd) = self.eventfd {
            let mut d = efd.lock();
            if d.counter < crate::task::process::EFD_MAX { d.counter += 1; }
            let key = d.key;
            drop(d);
            crate::task::scheduler::wake_pipe(key);
        }
    }

    pub fn drain_cqes(&mut self, buf: &mut [IoCqe]) -> usize {
        let mask = self.cq_ring.ring_mask as usize;
        let mut count = 0usize;
        let head = self.cq_ring.head as usize;
        let tail = self.cq_ring.tail as usize;
        let mut pos = head;
        while pos != tail && count < buf.len() {
            let idx = pos & mask;
            if idx < self.cq_entries.len() { buf[count] = self.cq_entries[idx]; }
            count += 1;
            pos += 1;
        }
        self.cq_ring.head = (head + count) as u32;
        count
    }

    pub fn peek_cqes(&self) -> u32 {
        self.cq_ring.tail.wrapping_sub(self.cq_ring.head)
    }
}

static NEXT_IO_URING_KEY: AtomicU64 = AtomicU64::new(0x6000_0000_0000);
pub fn next_io_uring_key() -> u64 {
    NEXT_IO_URING_KEY.fetch_add(1, Ordering::Relaxed)
}

// ── SQE processing ─────────────────────────────────────────────────

fn process_sqe(sqe: &IoSqEntry) -> IoCqe {
    let ud = sqe.user_data;
    match sqe.opcode {
        IORING_OP_NOP => IoCqe { user_data: ud, res: 0, flags: 0 },
        IORING_OP_READV => IoCqe { user_data: ud, res: do_readv(sqe.fd, sqe.addr, sqe.len as usize), flags: 0 },
        IORING_OP_WRITEV => IoCqe { user_data: ud, res: do_writev(sqe.fd, sqe.addr, sqe.len as usize), flags: 0 },
        IORING_OP_CLOSE => IoCqe { user_data: ud, res: do_close(sqe.fd), flags: 0 },
        IORING_OP_POLL_ADD => IoCqe { user_data: ud, res: do_poll_add(sqe.fd, sqe.len as u32), flags: 0 },
        IORING_OP_POLL_REMOVE => IoCqe { user_data: ud, res: 0, flags: 0 },
        _ => IoCqe { user_data: ud, res: Errno::ENOSYS as i32, flags: 0 },
    }
}

// ── Opcode implementations ──────────────────────────────────────────

fn do_readv(fd: i32, addr: u64, len: usize) -> i32 {
    if len == 0 || addr == 0 { return Errno::EINVAL as i32; }
    if !user_access::validate_ptr(addr as *const u8, len) { return Errno::EFAULT as i32; }
    let buf = unsafe { core::slice::from_raw_parts_mut(addr as *mut u8, len) };
    crate::syscalls::fs_io::sys_read(fd as u64, buf.as_mut_ptr(), len) as i32
}

fn do_writev(fd: i32, addr: u64, len: usize) -> i32 {
    if len == 0 || addr == 0 { return Errno::EINVAL as i32; }
    if !user_access::validate_ptr(addr as *const u8, len) { return Errno::EFAULT as i32; }
    let buf = unsafe { core::slice::from_raw_parts(addr as *const u8, len) };
    crate::syscalls::fs_io::sys_write(fd as u64, buf.as_ptr(), len) as i32
}

fn do_close(fd: i32) -> i32 {
    if fd < 0 { return Errno::EBADF as i32; }
    let ret = crate::syscalls::fs_open::sys_close(fd as u64);
    if ret == 0 { 0 } else { ret as i32 }
}

fn do_poll_add(fd: i32, poll_mask: u32) -> i32 {
    if fd < 0 { return Errno::EBADF as i32; }
    let proc = match *CURRENT_PROCESS.lock() {
        Some(ref p) => Arc::clone(p),
        None => return Errno::ESRCH as i32,
    };
    let files = proc.files.lock();
    if (fd as usize) >= files.fd_table.len() { return Errno::EBADF as i32; }
    let mut revents: u32 = 0;
    match files.fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, .. }) => {
            if poll_mask & 1 != 0 && node.stat().map(|s| s.st_size > 0).unwrap_or(false) { revents |= 1; }
            revents |= 4;
        }
        Some(FileDescriptor::Socket(..)) | Some(FileDescriptor::UnixSocket(..)) => { revents |= 4; }
        Some(_) => { revents |= 1 | 4; }
        None => return Errno::EBADF as i32,
    }
    drop(files);
    if (revents & poll_mask) != 0 { revents as i32 } else { Errno::EAGAIN as i32 }
}

// ── Syscalls ───────────────────────────────────────────────────────

pub fn sys_io_uring_setup(entries: u64, params_ptr: u64) -> u64 {
    let entries = entries as u32;
    if entries == 0 || entries > IORING_MAX_ENTRIES { return Errno::EINVAL as u64; }

    let instance = Arc::new(Mutex::new(IoUringInstance::new(entries)));
    let proc = match *CURRENT_PROCESS.lock() {
        Some(ref p) => Arc::clone(p),
        None => return Errno::ESRCH as u64,
    };
    let mut files = proc.files.lock();
    let fd_num = {
        let mut found = None;
        for (i, slot) in files.fd_table.iter().enumerate() {
            if slot.is_none() { found = Some(i); break; }
        }
        match found {
            Some(i) => { files.fd_table[i] = Some(FileDescriptor::IoUringFd(instance)); i }
            None => { files.fd_table.push(Some(FileDescriptor::IoUringFd(instance))); files.fd_table.len() - 1 }
        }
    };

    if params_ptr != 0 {
        // Write sq_entries + cq_entries back to userspace (first 8 bytes)
        let data = [entries, entries * 2];
        unsafe { let _ = user_access::copy_to_user(params_ptr as *mut u8, core::slice::from_raw_parts(data.as_ptr() as *const u8, 8)); }
    }
    fd_num as u64
}

pub fn sys_io_uring_enter(fd: u64, to_submit: u32, min_complete: u32, flags: u32, _sig_ptr: u64) -> u64 {
    let proc = match *CURRENT_PROCESS.lock() {
        Some(ref p) => Arc::clone(p),
        None => return Errno::ESRCH as u64,
    };
    let files = proc.files.lock();
    if fd as usize >= files.fd_table.len() { return Errno::EBADF as u64; }
    let instance_arc = match files.fd_table[fd as usize] {
        Some(FileDescriptor::IoUringFd(ref arc)) => Arc::clone(arc),
        _ => return Errno::EBADF as u64,
    };
    drop(files);

    let mut inst = instance_arc.lock();
    if to_submit > 0 { inst.process_all(); }

    if min_complete > 0 && (flags & IORING_ENTER_GETEVENTS != 0) {
        for _ in 0..10_000u64 {
            if inst.peek_cqes() >= min_complete { break; }
            crate::task::scheduler::try_schedule();
        }
    }
    inst.peek_cqes() as u64
}

pub fn sys_io_uring_register(fd: u64, opcode: u32, arg: u64, nr_args: u32) -> u64 {
    let proc = match *CURRENT_PROCESS.lock() {
        Some(ref p) => Arc::clone(p),
        None => return Errno::ESRCH as u64,
    };
    let files = proc.files.lock();
    if fd as usize >= files.fd_table.len() { return Errno::EBADF as u64; }
    let instance_arc = match files.fd_table[fd as usize] {
        Some(FileDescriptor::IoUringFd(ref arc)) => Arc::clone(arc),
        _ => return Errno::EBADF as u64,
    };
    drop(files);

    let mut inst = instance_arc.lock();
    match opcode {
        IORING_REGISTER_BUFFERS => {
            if arg == 0 || nr_args == 0 || nr_args > 1024 { return Errno::EINVAL as u64; }
            #[repr(C)] struct IoVec { addr: u64, len: u64 }
            let iovecs = unsafe { core::slice::from_raw_parts(arg as *const IoVec, nr_args as usize) };
            inst.fixed_buffers = iovecs.iter().map(|iov| (iov.addr, iov.len as u32)).collect();
            0
        }
        IORING_UNREGISTER_BUFFERS => { inst.fixed_buffers.clear(); 0 }
        IORING_REGISTER_FILES => {
            if arg == 0 || nr_args == 0 || nr_args > 1024 { return Errno::EINVAL as u64; }
            let fds = unsafe { core::slice::from_raw_parts(arg as *const i32, nr_args as usize) };
            inst.fixed_files = fds.to_vec();
            0
        }
        IORING_UNREGISTER_FILES => { inst.fixed_files.clear(); 0 }
        IORING_REGISTER_EVENTFD => {
            if arg == 0 { return Errno::EINVAL as u64; }
            let efd_fd = unsafe { core::ptr::read_volatile(arg as *const i32) };
            if efd_fd < 0 { return Errno::EINVAL as u64; }
            let proc2 = match *CURRENT_PROCESS.lock() {
                Some(ref p) => Arc::clone(p),
                None => return Errno::ESRCH as u64,
            };
            let files2 = proc2.files.lock();
            if (efd_fd as usize) < files2.fd_table.len() {
                if let Some(FileDescriptor::EventFd(ref efd_arc)) = files2.fd_table[efd_fd as usize] {
                    inst.eventfd = Some(Arc::clone(efd_arc));
                    drop(files2);
                    return 0;
                }
            }
            Errno::EBADF as u64
        }
        IORING_UNREGISTER_EVENTFD => { inst.eventfd = None; 0 }
        _ => Errno::EINVAL as u64,
    }
}
