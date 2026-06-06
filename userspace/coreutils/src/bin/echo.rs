#![no_std]
#![no_main]
extern crate alloc;
use coreutils::print;
#[global_allocator]
static A: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    let mut newline = true;
    let mut start = 1;
    if _argc > 1 {
        let first = unsafe { core::ffi::CStr::from_ptr(*_argv.add(1) as *const i8).to_str().unwrap_or("") };
        if first == "-n" { newline = false; start = 2; }
    }
    for i in start.._argc {
        let s = unsafe { core::ffi::CStr::from_ptr(*_argv.add(i as usize) as *const i8).to_str().unwrap_or("") };
        if i > start { print(" "); }
        print(s);
    }
    if newline { print("\n"); }
    0
}
