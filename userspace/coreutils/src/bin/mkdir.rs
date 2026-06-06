#![no_std]
#![no_main]

extern crate alloc;
use coreutils::{eprint, cstr};

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    if _argc < 2 {
        eprint("mkdir: missing operand\n");
        return 1;
    }
    for i in 1.._argc {
        let path = unsafe {
            let ptr = *_argv.add(i as usize);
            if ptr.is_null() { continue; }
            core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("")
        };
        let c = cstr(path);
        let ret = skyos_libc::syscall::mkdir(c.as_ptr() as *const u8, 0o755);
        if ret >= 0xFFFF_FFFF_FFFF_FF00 {
            eprint("mkdir: cannot create "); eprint(path); eprint("\n");
        }
    }
    0
}
