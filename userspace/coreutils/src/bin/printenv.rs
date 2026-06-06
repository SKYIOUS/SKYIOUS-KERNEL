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
    if _argc >= 2 {
        if let Some(var) = core::option_env!("PATH") {
            let line = alloc::format!("PATH={}\n", var);
            print(&line);
        }
        return 0;
    }
    for (k, v) in [
        ("HOME", "/home/root"),
        ("PATH", "/bin:/sbin"),
        ("TERM", "skycon"),
        ("SHELL", "/bin/sargash"),
    ] {
        let line = alloc::format!("{}={}\n", k, v);
        print(&line);
    }
    0
}
