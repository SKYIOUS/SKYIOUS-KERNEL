#![no_std]
#![no_main]
extern crate alloc;
use coreutils::{eprint, open_read, print};
use alloc::vec::Vec;
use alloc::string::String;
#[global_allocator]
static A: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    if _argc < 2 { eprint("grep: missing pattern\n"); return 1; }
    let pattern = unsafe { core::ffi::CStr::from_ptr(*_argv.add(1) as *const i8).to_str().unwrap_or("") };
    let mut exit_code = 1;
    let read_stdin = _argc < 3;
    if read_stdin {
        let mut buf = [0u8; 4096];
        let content = loop {
            let nr = skyos_libc::syscall::read(0, &mut buf);
            if (nr as i64) <= 0 { break String::new(); }
            break String::from_utf8_lossy(&buf[..nr as usize]).into_owned();
        };
        for line in content.lines() {
            if line.contains(pattern) { print(line); print("\n"); exit_code = 0; }
        }
        return exit_code;
    }
    for fi in 2.._argc {
        let p = unsafe { core::ffi::CStr::from_ptr(*_argv.add(fi as usize) as *const i8).to_str().unwrap_or("") };
        let fd = match open_read(p) { Some(f) => f, None => { eprint("grep: "); eprint(p); eprint("\n"); continue; } };
        let mut buf = [0u8; 4096];
        let mut leftover = Vec::new();
        loop {
            let nr = skyos_libc::syscall::read(fd as u64, &mut buf);
            if (nr as i64) <= 0 { break; }
            leftover.extend_from_slice(&buf[..nr as usize]);
        }
        skyos_libc::syscall::close(fd as u64);
        let content = core::str::from_utf8(&leftover).unwrap_or("");
        for line in content.lines() {
            if line.contains(pattern) {
                if _argc > 3 { print(p); print(":"); }
                print(line); print("\n"); exit_code = 0;
            }
        }
    }
    exit_code
}
