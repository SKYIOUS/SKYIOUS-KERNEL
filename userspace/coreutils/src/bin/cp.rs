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
    if _argc < 3 {
        eprint("cp: missing operand\n");
        return 1;
    }
    let src = unsafe {
        let ptr = *_argv.add(1);
        core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("")
    };
    let dst = unsafe {
        let ptr = *_argv.add(2);
        core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("")
    };
    let c_src = cstr(src);
    let fd_r = skyos_libc::syscall::open(c_src.as_ptr() as *const u8, 0);
    if fd_r >= 0xFFFF_FFFF_FFFF_FF00 {
        eprint("cp: cannot open "); eprint(src); eprint("\n"); return 1;
    }
    let c_dst = cstr(dst);
    let fd_w = skyos_libc::syscall::open(c_dst.as_ptr() as *const u8, 0x0201 | 0x0040);
    if fd_w >= 0xFFFF_FFFF_FFFF_FF00 {
        eprint("cp: cannot create "); eprint(dst); eprint("\n");
        skyos_libc::syscall::close(fd_r);
        return 1;
    }
    let mut buf = [0u8; 4096];
    loop {
        let n = skyos_libc::syscall::read(fd_r, &mut buf);
        if n >= 0xFFFF_FFFF_FFFF_FF00 || n == 0 { break; }
        skyos_libc::syscall::write(fd_w, &buf[..n as usize]);
    }
    skyos_libc::syscall::close(fd_r);
    skyos_libc::syscall::close(fd_w);
    0
}
