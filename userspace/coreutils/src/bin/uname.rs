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
    let mut buf = [0u8; 512];
    let ret = skyos_libc::syscall::syscall1(skyos_libc::SYS_UNAME, buf.as_mut_ptr() as u64);
    if ret >= 0xFFFF_FFFF_FFFF_FF00 {
        print("SkyOS\n");
        return 0;
    }
    let start = 65 + 65 + 65;
    let version_end = start + buf[start..].iter().position(|&b| b == 0).unwrap_or(64);
    let version = core::str::from_utf8(&buf[start..start+version_end]).unwrap_or("SkyOS");
    let sysname = core::str::from_utf8(&buf[..buf.iter().position(|&b| b == 0).unwrap_or(64)]).unwrap_or("SkyOS");
    let release_start = 65;
    let release_end = release_start + buf[release_start..].iter().position(|&b| b == 0).unwrap_or(64);
    let release = core::str::from_utf8(&buf[release_start..release_start+release_end]).unwrap_or("0.1");
    print(sysname); print(" "); print(release); print(" "); print(version); print("\n");
    0
}
