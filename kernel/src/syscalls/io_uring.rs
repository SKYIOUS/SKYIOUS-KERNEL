use alloc::vec::Vec;
use alloc::boxed::Box;
use core::slice;
use spin::Mutex;
use crate::syscalls::errno::Errno;

// ── io_uring opcodes ─────────────────────────────────────────────────
pub const IORING_OP_NOP: u8 = 0;
pub const IORING_OP_READV: u8 = 1;
pub const IORING_OP_WRITEV: u8 = 2;
pub const IORING_OP_FSYNC: u8 = 3;
pub const IORING_OP_ACCEPT: u8 = 13;
pub const IORING_OP_CONNECT: u8 = 14;
pub const IORING_OP_SEND: u8 = 17;
pub const IORING_OP_RECV: u8 = 18;
pub const IORING_OP_TIMEOUT: u8 = 19;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoUringSqe {
    pub opcode: u8,
    pub flags: u8,
    pub ioprio: u16,
    pub fd: i32,
    pub off: u64,
    pub addr: u64,
    pub len: u32,
    pub user_data: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoUringCqe {
    pub user_data: u64,
    pub res: i32,
    pub flags: u32,
}

pub struct IoUring {
    pub sqes: Vec<IoUringSqe>,
    pub cqes: Vec<IoUringCqe>,
    pub sq_head: usize,
    pub cq_head: usize,
    pub entries: usize,
}

impl IoUring {
    pub fn new(entries: usize) -> Self {
        IoUring {
            sqes: Vec::with_capacity(entries),
            cqes: Vec::with_capacity(entries),
            sq_head: 0,
            cq_head: 0,
            entries,
        }
    }

    pub fn submit_sqes(&mut self, sqes: &[IoUringSqe]) -> usize {
        let avail = self.entries - self.sqes.len();
        let count = sqes.len().min(avail);
        self.sqes.extend_from_slice(&sqes[..count]);
        count
    }

    pub fn process_all(&mut self) {
        while self.sq_head < self.sqes.len() {
            let sqe = &self.sqes[self.sq_head];
            let cqe = process_sqe(sqe);
            self.cqes.push(cqe);
            self.sq_head += 1;
        }
        self.sqes.clear();
        self.sq_head = 0;
    }

    pub fn copy_cqes(&self, buf: &mut [u8]) -> usize {
        let count = self.cqes.len();
        if count == 0 { return 0; }
        let cqe_size = core::mem::size_of::<IoUringCqe>();
        let bytes = count * cqe_size;
        let copy_len = bytes.min(buf.len());
        let src = unsafe {
            slice::from_raw_parts(self.cqes.as_ptr() as *const u8, copy_len)
        };
        buf[..copy_len].copy_from_slice(src);
        copy_len / cqe_size
    }
}

fn process_sqe(sqe: &IoUringSqe) -> IoUringCqe {
    match sqe.opcode {
        IORING_OP_NOP => IoUringCqe { user_data: sqe.user_data, res: 0, flags: 0 },
        IORING_OP_READV => {
            let res = do_readv(sqe.fd, sqe.addr, sqe.len as usize, sqe.off);
            IoUringCqe { user_data: sqe.user_data, res, flags: 0 }
        }
        IORING_OP_WRITEV => {
            let res = do_writev(sqe.fd, sqe.addr, sqe.len as usize, sqe.off);
            IoUringCqe { user_data: sqe.user_data, res, flags: 0 }
        }
        IORING_OP_FSYNC => {
            let res = do_fsync(sqe.fd);
            IoUringCqe { user_data: sqe.user_data, res, flags: 0 }
        }
        IORING_OP_ACCEPT => {
            let res = do_accept(sqe.fd, sqe.addr, sqe.len);
            IoUringCqe { user_data: sqe.user_data, res, flags: 0 }
        }
        IORING_OP_CONNECT => {
            let res = do_connect(sqe.fd, sqe.addr, sqe.len as usize);
            IoUringCqe { user_data: sqe.user_data, res, flags: 0 }
        }
        IORING_OP_SEND => {
            let res = do_send(sqe.fd, sqe.addr, sqe.len as usize);
            IoUringCqe { user_data: sqe.user_data, res, flags: 0 }
        }
        IORING_OP_RECV => {
            let res = do_recv(sqe.fd, sqe.addr, sqe.len as usize);
            IoUringCqe { user_data: sqe.user_data, res, flags: 0 }
        }
        IORING_OP_TIMEOUT => {
            let res = do_timeout(sqe.addr);
            IoUringCqe { user_data: sqe.user_data, res, flags: 0 }
        }
        _ => IoUringCqe { user_data: sqe.user_data, res: Errno::ENOSYS as i32, flags: 0 },
    }
}

fn do_readv(fd: i32, addr: u64, len: usize, _offset: u64) -> i32 {
    if len == 0 || addr == 0 { return Errno::EINVAL as i32; }
    let buf = unsafe { slice::from_raw_parts_mut(addr as *mut u8, len) };
    let ret = crate::syscalls::sys_read(fd as u64, buf.as_mut_ptr() as *mut u8, len);
    ret as i32
}

fn do_writev(fd: i32, addr: u64, len: usize, _offset: u64) -> i32 {
    if len == 0 || addr == 0 { return Errno::EINVAL as i32; }
    let buf = unsafe { slice::from_raw_parts(addr as *const u8, len) };
    let ret = crate::syscalls::sys_write(fd as u64, buf.as_ptr() as *const u8, len);
    ret as i32
}

fn do_fsync(_fd: i32) -> i32 { 0 }

fn do_accept(fd: i32, addr: u64, addrlen: u32) -> i32 {
    let addrlen_ptr = if addrlen > 0 && addr != 0 {
        &addrlen as *const u32 as *mut u32
    } else {
        core::ptr::null_mut()
    };
    let ret = crate::syscalls::sys_accept(fd as u64, addr as *mut u8, addrlen_ptr);
    if (ret as i64) < 0 { ret as i32 } else { ret as i32 }
}

fn do_connect(fd: i32, addr: u64, addrlen: usize) -> i32 {
    let ret = crate::syscalls::sys_connect(fd as u64, addr as *const u8, addrlen as u64);
    if (ret as i64) < 0 { ret as i32 } else { ret as i32 }
}

fn do_send(fd: i32, addr: u64, len: usize) -> i32 {
    let buf = unsafe { slice::from_raw_parts(addr as *const u8, len) };
    let ret = crate::syscalls::sys_sendto(fd as u64, buf.as_ptr() as *const u8, len as u64, core::ptr::null(), 0);
    if (ret as i64) < 0 { ret as i32 } else { ret as i32 }
}

fn do_recv(fd: i32, addr: u64, len: usize) -> i32 {
    let buf = unsafe { slice::from_raw_parts_mut(addr as *mut u8, len) };
    let ret = crate::syscalls::sys_recvfrom(fd as u64, buf.as_mut_ptr() as *mut u8, len as u64, core::ptr::null_mut(), core::ptr::null_mut());
    if (ret as i64) < 0 { ret as i32 } else { ret as i32 }
}

fn do_timeout(addr: u64) -> i32 {
    if addr == 0 { return Errno::EINVAL as i32; }
    let ts = unsafe { (addr as *const crate::syscalls::Timespec).read() };
    let ms = ts.tv_sec as u64 * 1000 + ts.tv_nsec as u64 / 1_000_000;
    let ticks = (ms / 10).max(1);
    let start = crate::interrupts::get_ticks();
    while crate::interrupts::get_ticks() - start < ticks {
        crate::task::scheduler::try_schedule();
    }
    0
}

pub fn sys_io_uring_setup(entries: u64) -> u64 {
    let entries = entries as usize;
    if entries == 0 || entries > 4096 {
        return Errno::EINVAL as u64;
    }
    let ring = Box::new(Mutex::new(IoUring::new(entries)));
    let ptr = Box::into_raw(ring) as u64;
    let proc_lock = crate::task::process::CURRENT_PROCESS.lock();
    if let Some(ref proc) = *proc_lock {
        let mut io_rings = proc.io_rings.lock();
        let fd = 200 + io_rings.len() as u64;
        io_rings.push((ptr, entries));
        fd
    } else {
        Errno::ENOSYS as u64
    }
}

pub fn sys_io_uring_enter(fd: u64, to_submit: u64, min_complete: u64, sqe_ptr: u64, cqe_ptr: u64) -> u64 {
    if fd < 200 { return Errno::EBADF as u64; }
    let idx = (fd - 200) as usize;
    let (ring_ptr, _entries) = {
        let proc_lock = crate::task::process::CURRENT_PROCESS.lock();
        let proc = match *proc_lock {
            Some(ref proc) => proc,
            None => return Errno::ENOSYS as u64,
        };
        let io_rings = proc.io_rings.lock();
        if idx >= io_rings.len() {
            return Errno::EBADF as u64;
        }
        io_rings[idx]
    };

    let ring = unsafe { &mut *(ring_ptr as *mut Mutex<IoUring>) };
    let mut ring = ring.lock();

    if to_submit > 0 && sqe_ptr != 0 {
        let total = to_submit as usize;
        let sqes = unsafe { slice::from_raw_parts(sqe_ptr as *const IoUringSqe, total) };
        let submitted = ring.submit_sqes(sqes);
        if submitted > 0 {
            ring.process_all();
        }
    }

    let completed = ring.cqes.len();
    if cqe_ptr != 0 && completed > 0 {
        let cqe_size = core::mem::size_of::<IoUringCqe>();
        let cqe_buf = unsafe {
            slice::from_raw_parts_mut(cqe_ptr as *mut u8, completed * cqe_size)
        };
        let n_cqes = ring.copy_cqes(cqe_buf);
        ring.cqes.clear();
        ring.cq_head = 0;
        if n_cqes > 0 {
            return n_cqes as u64;
        }
    }

    if min_complete > 0 {
        let max_spins = 10_000u64;
        for _ in 0..max_spins {
            crate::task::scheduler::try_schedule();
            let n = ring.cqes.len();
            if n >= min_complete as usize {
                if cqe_ptr != 0 && n > 0 {
                    let cqe_size = core::mem::size_of::<IoUringCqe>();
                    let cqe_buf = unsafe {
                        slice::from_raw_parts_mut(cqe_ptr as *mut u8, n * cqe_size)
                    };
                    let n_cqes = ring.copy_cqes(cqe_buf);
                    ring.cqes.clear();
                    ring.cq_head = 0;
                    return n_cqes as u64;
                }
                return n as u64;
            }
        }
    }

    completed as u64
}
