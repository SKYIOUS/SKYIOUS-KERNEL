#![no_std]
#![no_main]
extern crate alloc;
use coreutils::{eprint, open_read, print};
use alloc::vec::Vec;
#[global_allocator]
static A: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }
fn read_all(fd: i64) -> Vec<u8> {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = skyos_libc::syscall::read(fd as u64, &mut buf);
        if (n as i64) <= 0 { break; }
        data.extend_from_slice(&buf[..n as usize]);
    }
    data
}
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    if _argc < 2 {
        let data = read_all(0);
        let content = core::str::from_utf8(&data).unwrap_or("");
        let mut lines: Vec<&str> = content.lines().collect();
        lines.sort();
        for l in lines { print(l); print("\n"); }
        return 0;
    }
    for i in 1.._argc {
        let p = unsafe { core::ffi::CStr::from_ptr(*_argv.add(i as usize) as *const i8).to_str().unwrap_or("") };
        let fd = match open_read(p) { Some(f) => f, None => { eprint("sort: "); eprint(p); eprint("\n"); continue; } };
        let data = read_all(fd);
        skyos_libc::syscall::close(fd as u64);
        let content = core::str::from_utf8(&data).unwrap_or("");
        let mut lines: Vec<&str> = content.lines().collect();
        lines.sort();
        for l in lines { print(l); print("\n"); }
    }
    0
}
