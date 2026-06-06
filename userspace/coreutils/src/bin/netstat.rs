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
    print("Active Internet connections\n");
    print("Proto  Recv-Q  Send-Q  Local Address  Foreign Address  State\n");
    print("(no kernel instrumention yet)\n");
    0
}
