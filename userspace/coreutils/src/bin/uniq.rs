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
    let fd = if _argc < 2 { 0 } else {
        let p = unsafe { core::ffi::CStr::from_ptr(*_argv.add(1) as *const i8).to_str().unwrap_or("") };
        match open_read(p) { Some(f) => f, None => { eprint("uniq: "); eprint(p); eprint("\n"); return 1; } }
    };
    let mut buf = [0u8; 4096];
    let mut prev = Vec::new();
    loop {
        let n = skyos_libc::syscall::read(fd as u64, &mut buf);
        if (n as i64) <= 0 { break; }
        let content = core::str::from_utf8(&buf[..n as usize]).unwrap_or("");
        for line in content.lines() {
            let cur = line.as_bytes();
            if cur != prev.as_slice() {
                print(line); print("\n");
                prev = cur.to_vec();
            }
        }
    }
    if _argc >= 2 { skyos_libc::syscall::close(fd as u64); }
    0
}
