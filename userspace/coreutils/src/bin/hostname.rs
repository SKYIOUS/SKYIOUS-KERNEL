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
    let mut buf = [0u8; 512];
    let ret = skyos_libc::syscall::syscall1(skyos_libc::SYS_UNAME, buf.as_mut_ptr() as u64);
    if ret >= 0xFFFF_FFFF_FFFF_FF00 { print("skyos\n"); return 0; }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(64);
    print(core::str::from_utf8(&buf[..len]).unwrap_or("skyos"));
    print("\n");
    0
}
