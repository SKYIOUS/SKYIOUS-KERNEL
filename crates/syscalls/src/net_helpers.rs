//! Network syscall helpers: types and constants.
//!
//! Stub module for crate extraction. Full implementation lives in
//! `kernel/src/syscalls/net_helpers.rs`.

/// I/O vector structure for readv/writev.
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct iovec {
    pub iov_base: *mut u8,
    pub iov_len: usize,
}

/// Message header for sendmsg/recvmsg.
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct msghdr {
    pub msg_name: *mut u8,
    pub msg_namelen: u32,
    pub msg_iov: *const iovec,
    pub msg_iovlen: usize,
    pub msg_control: *mut u8,
    pub msg_controllen: usize,
    pub msg_flags: i32,
}
