#![no_std]
#![no_main]

extern crate alloc;
use coreutils::print;

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    let msg = if _argc < 2 { "y" } else {
        unsafe {
            let ptr = *_argv.add(1);
            if ptr.is_null() { "y" } else {
                core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("y")
            }
        }
    };
    loop {
        print(msg);
        print("\n");
    }
}
