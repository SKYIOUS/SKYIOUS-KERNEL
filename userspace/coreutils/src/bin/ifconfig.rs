#![no_std]
#![no_main]
extern crate alloc;
use coreutils::print;
use alloc::string::String;
#[global_allocator]
static A: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    let mut buf = [0u8; 512];
    let ret = skyos_libc::syscall::syscall1(63, buf.as_mut_ptr() as u64);
    if ret >= 0xFFFF_FFFF_FFFF_FF00 { print("SkyOS\n"); return 0; }
    let sysname_end = buf.iter().position(|&b| b == 0).unwrap_or(64);
    let release_start = 65;
    let release_end = release_start + buf[release_start..].iter().position(|&b| b == 0).unwrap_or(64);
    let release = core::str::from_utf8(&buf[release_start..release_start+release_end]).unwrap_or("0.1");
    let nodename_start = 65 + 65;
    let nodename_end = nodename_start + buf[nodename_start..].iter().position(|&b| b == 0).unwrap_or(64);
    let nodename = core::str::from_utf8(&buf[nodename_start..nodename_start+nodename_end]).unwrap_or("skyos");
    let msg = alloc::format!("{}  IP: 10.0.2.15  Mask: 255.255.255.0  MAC: (from NIC)\n", nodename);
    print(&msg);
    let hw = alloc::format!("      RX packets: 0  TX packets: 0\n");
    print(&hw);
    0
}
