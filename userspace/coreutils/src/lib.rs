#![no_std]

extern crate alloc;

use alloc::ffi::CString;

pub fn cstr(s: &str) -> CString {
    CString::new(s.as_bytes()).unwrap()
}

pub fn eprint(s: &str) {
    let _ = skyos_libc::syscall::write(2, s.as_bytes());
}

pub fn print(s: &str) {
    let _ = skyos_libc::syscall::write(1, s.as_bytes());
}

pub fn open_read(path: &str) -> Option<i64> {
    let c = cstr(path);
    let fd = skyos_libc::syscall::open(c.as_ptr() as *const u8, 0);
    if fd >= 0xFFFF_FFFF_FFFF_FF00 { None } else { Some(fd as i64) }
}

pub fn open_write(path: &str) -> Option<i64> {
    let c = cstr(path);
    let fd = skyos_libc::syscall::open(c.as_ptr() as *const u8, 0x0201 | 0x0040);
    if fd >= 0xFFFF_FFFF_FFFF_FF00 { None } else { Some(fd as i64) }
}

pub fn stat(path: &str, buf: &mut [u8]) -> bool {
    let c = cstr(path);
    skyos_libc::syscall::stat(c.as_ptr() as *const u8, buf.as_mut_ptr()) < 0xFFFF_FFFF_FFFF_FF00
}

pub fn fstat(fd: i64, buf: &mut [u8]) -> bool {
    skyos_libc::syscall::fstat(fd as u64, buf.as_mut_ptr()) < 0xFFFF_FFFF_FFFF_FF00
}

pub fn getdents(fd: i64, buf: &mut [u8]) -> usize {
    let n = skyos_libc::syscall::getdents64(fd as u64, buf.as_mut_ptr(), buf.len());
    if n >= 0xFFFF_FFFF_FFFF_FF00 { 0 } else { n as usize }
}
