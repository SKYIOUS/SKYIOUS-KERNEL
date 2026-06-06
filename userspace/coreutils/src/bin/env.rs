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
    let envp: *const *const u8 = unsafe { _argv.add((_argc as usize) + 1) };
    if !envp.is_null() {
        let mut i = 0;
        while !unsafe { (*envp.add(i)).is_null() } {
            let s = unsafe { core::ffi::CStr::from_ptr(*envp.add(i) as *const i8).to_str().unwrap_or("") };
            print(s); print("\n");
            i += 1;
        }
    }
    0
}
