#![no_std]
#![no_main]
extern crate alloc;
use coreutils::{eprint, open_read, print};
use alloc::vec::Vec;
#[global_allocator]
static A: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    let mut n: usize = 10;
    let mut start = 1;
    if _argc > 1 {
        let first = unsafe { core::ffi::CStr::from_ptr(*_argv.add(1) as *const i8).to_str().unwrap_or("") };
        if first.len() > 1 && first.as_bytes()[0] == b'-' {
            if let Ok(num) = first[1..].parse::<usize>() { n = num; start = 2; }
        }
    }
    for i in start.._argc {
        let p = unsafe { core::ffi::CStr::from_ptr(*_argv.add(i as usize) as *const i8).to_str().unwrap_or("") };
        let fd = match open_read(p) { Some(f) => f, None => { eprint("tail: "); eprint(p); eprint("\n"); continue; } };
        let mut buf = alloc::vec![0u8; 4096];
        let nr = skyos_libc::syscall::read(fd as u64, &mut buf);
        skyos_libc::syscall::close(fd as u64);
        if (nr as i64) <= 0 { continue; }
        let data = &buf[..nr as usize];
        let mut newlines = Vec::new();
        for (j, &b) in data.iter().enumerate() { if b == b'\n' { newlines.push(j); } }
        let start_line = if newlines.len() > n { newlines.len() - n } else { 0 };
        let start_pos = if start_line == 0 { 0 } else { newlines[start_line - 1] + 1 };
        let s = core::str::from_utf8(&data[start_pos..]).unwrap_or("");
        print(s);
        if !s.ends_with('\n') { print("\n"); }
    }
    0
}
