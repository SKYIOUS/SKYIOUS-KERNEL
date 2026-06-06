#![no_std]
#![no_main]
extern crate alloc;
use coreutils::{eprint, open_read, print};
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
        let fd = match open_read(p) { Some(f) => f, None => { eprint("head: "); eprint(p); eprint("\n"); continue; } };
        let mut buf = [0u8; 4096];
        let mut lines = 0;
        loop {
            let nr = skyos_libc::syscall::read(fd as u64, &mut buf);
            if (nr as i64) <= 0 { break; }
            for &b in &buf[..nr as usize] {
                if lines >= n { break; }
                let byte_slice = [b];
                let c = core::str::from_utf8(&byte_slice).unwrap_or("");
                print(c);
                if b == b'\n' { lines += 1; }
            }
            if lines >= n { break; }
        }
        skyos_libc::syscall::close(fd as u64);
    }
    0
}
