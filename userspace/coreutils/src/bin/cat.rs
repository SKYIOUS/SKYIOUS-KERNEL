#![no_std]
#![no_main]

extern crate alloc;
use coreutils::{eprint, open_read};

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    if _argc < 2 {
        let mut buf = [0u8; 4096];
        loop {
            let n = skyos_libc::syscall::read(0, &mut buf);
            if n >= 0xFFFF_FFFF_FFFF_FF00 || n == 0 { break; }
            skyos_libc::syscall::write(1, &buf[..n as usize]);
        }
        return 0;
    }
    for i in 1.._argc {
        let path = unsafe {
            let ptr = *_argv.add(i as usize);
            if ptr.is_null() { continue; }
            core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("")
        };
        let fd = match open_read(path) {
            Some(f) => f,
            None => { eprint("cat: "); eprint(path); eprint(": not found\n"); continue; }
        };
        let mut buf = [0u8; 4096];
        loop {
            let n = skyos_libc::syscall::read(fd as u64, &mut buf);
            if n >= 0xFFFF_FFFF_FFFF_FF00 || n == 0 { break; }
            skyos_libc::syscall::write(1, &buf[..n as usize]);
        }
        skyos_libc::syscall::close(fd as u64);
    }
    0
}
