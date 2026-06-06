#![no_std]
#![no_main]

extern crate alloc;
use coreutils::{eprint, print, open_read, getdents, fstat};

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    let path = if _argc > 1 {
        unsafe {
            let ptr = *_argv.add(1);
            if ptr.is_null() { "/" } else {
                core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("/")
            }
        }
    } else { "/" };

    let fd = match open_read(path) {
        Some(f) => f,
        None => { eprint("ls: cannot access "); eprint(path); eprint("\n"); return 1; }
    };

    let mut st = [0u8; 128];
    if !fstat(fd, &mut st) { return 1; }
    let st_mode = u32::from_ne_bytes(st[..4].try_into().unwrap_or([0; 4]));

    if st_mode & 0o170000 == 0o040000 {
        let mut buf = [0u8; 4096];
        let n = getdents(fd, &mut buf);
        let mut off = 0;
        while off + 18 < n {
            let reclen = u16::from_ne_bytes(buf[off+16..off+18].try_into().unwrap()) as usize;
            if reclen < 19 || off + reclen > n { break; }
            let name_end = off + 18 + buf[off+18..off+reclen].iter().position(|&b| b == 0).unwrap_or(reclen - 19);
            let name = core::str::from_utf8(&buf[off+18..name_end]).unwrap_or("");
            if name != "." && name != ".." {
                print(name); print("  ");
            }
            off += reclen;
        }
        print("\n");
    } else {
        print(path); print("\n");
    }

    skyos_libc::syscall::close(fd as u64);
    0
}
