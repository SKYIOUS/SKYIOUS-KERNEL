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
    for i in 1.._argc {
        let p = unsafe { core::ffi::CStr::from_ptr(*_argv.add(i as usize) as *const i8).to_str().unwrap_or("") };
        let c = cstr(p);
        let fd = skyos_libc::syscall::open(c.as_ptr() as *const u8, 0x41);
        if fd >= 0xFFFF_FFFF_FFFF_FF00 { eprint("touch: "); eprint(p); eprint("\n"); }
        else { skyos_libc::syscall::close(fd); }
    }
    0
}
