#![no_std]
#![no_main]
extern crate alloc;
use coreutils::{eprint, print};
use libskyos::net;
#[global_allocator]
static A: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    if _argc < 2 { eprint("Usage: nslookup <hostname>\n"); return 1; }
    let host = unsafe { core::ffi::CStr::from_ptr(*_argv.add(1) as *const i8).to_str().unwrap_or("") };
    match net::resolve(host) {
        Some(ip) => { let msg = alloc::format!("{} resolves to {}\n", host, ip); print(&msg); 0 }
        None => { eprint("nslookup: could not resolve "); eprint(host); eprint("\n"); 1 }
    }
}
