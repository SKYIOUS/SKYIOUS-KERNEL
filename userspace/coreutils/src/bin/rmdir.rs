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
    if _argc < 2 { eprint("rmdir: missing operand\n"); return 1; }
    for i in 1.._argc {
        let p = unsafe { core::ffi::CStr::from_ptr(*_argv.add(i as usize) as *const i8).to_str().unwrap_or("") };
        let ret = skyos_libc::syscall::unlink(cstr(p).as_ptr() as *const u8);
        if ret >= 0xFFFF_FFFF_FFFF_FF00 { eprint("rmdir: "); eprint(p); eprint("\n"); }
    }
    0
}
