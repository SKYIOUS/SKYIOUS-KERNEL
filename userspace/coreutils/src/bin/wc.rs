#![no_std]
#![no_main]
extern crate alloc;
use coreutils::{eprint, open_read, print};
#[global_allocator]
static A: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }
fn count(fd: i64) -> (usize, usize, usize) {
    let mut lines = 0usize;
    let mut words = 0usize;
    let mut chars = 0usize;
    let mut in_word = false;
    let mut buf = [0u8; 4096];
    loop {
        let nr = skyos_libc::syscall::read(fd as u64, &mut buf);
        if (nr as i64) <= 0 { break; }
        for &b in &buf[..nr as usize] {
            chars += 1;
            if b == b'\n' { lines += 1; }
            if b == b' ' || b == b'\n' || b == b'\t' { in_word = false; }
            else if !in_word { words += 1; in_word = true; }
        }
    }
    (lines, words, chars)
}
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    if _argc < 2 {
        let (l, w, c) = count(0);
        let s = alloc::format!("{} {} {}\n", l, w, c); print(&s);
        return 0;
    }
    for i in 1.._argc {
        let p = unsafe { core::ffi::CStr::from_ptr(*_argv.add(i as usize) as *const i8).to_str().unwrap_or("") };
        let fd = match open_read(p) { Some(f) => f, None => { eprint("wc: "); eprint(p); eprint("\n"); continue; } };
        let (l, w, c) = count(fd);
        let s = alloc::format!("{} {} {} {}\n", l, w, c, p); print(&s);
        skyos_libc::syscall::close(fd as u64);
    }
    0
}
