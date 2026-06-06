#![no_std]
#![no_main]
extern crate alloc;
use coreutils::{eprint, cstr};
#[global_allocator]
static A: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    if _argc < 3 { eprint("ln: missing operand\n"); return 1; }
    let target = unsafe { core::ffi::CStr::from_ptr(*_argv.add(1) as *const i8).to_str().unwrap_or("") };
    let link = unsafe { core::ffi::CStr::from_ptr(*_argv.add(2) as *const i8).to_str().unwrap_or("") };
    let ret = skyos_libc::syscall::syscall2(skyos_libc::SYS_SYMLINK, cstr(target).as_ptr() as u64, cstr(link).as_ptr() as u64);
    if ret >= 0xFFFF_FFFF_FFFF_FF00 { eprint("ln: failed\n"); return 1; }
    0
}
