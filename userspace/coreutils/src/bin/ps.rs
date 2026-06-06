#![no_std]
#![no_main]

extern crate alloc;
use coreutils::{eprint, print, open_read, getdents, cstr};

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    let fd = match open_read("/proc") {
        Some(f) => f,
        None => { eprint("ps: /proc not mounted\n"); return 1; }
    };
    print("PID   CMD\n");
    let mut buf = [0u8; 4096];
    let n = getdents(fd, &mut buf);
    let mut off = 0;
    while off + 18 < n {
        let reclen = u16::from_ne_bytes(buf[off+16..off+18].try_into().unwrap()) as usize;
        if reclen < 19 || off + reclen > n { break; }
        let name_end = off + 18 + buf[off+18..off+reclen].iter().position(|&b| b == 0).unwrap_or(reclen - 19);
        let name = core::str::from_utf8(&buf[off+18..name_end]).unwrap_or("");
        if let Ok(pid) = name.parse::<i64>() {
            let cmd_path = alloc::format!("/proc/{}/cmdline\0", pid);
            let c = cstr(&cmd_path);
            let cmd_fd = skyos_libc::syscall::open(c.as_ptr() as *const u8, 0);
            if cmd_fd < 0xFFFF_FFFF_FFFF_FF00 {
                let mut cmd_buf = [0u8; 128];
                let n2 = skyos_libc::syscall::read(cmd_fd, &mut cmd_buf);
                skyos_libc::syscall::close(cmd_fd);
                if n2 > 0 {
                    let cmd = core::str::from_utf8(&cmd_buf[..n2 as usize]).unwrap_or("");
                    let line = alloc::format!("{:<5} {}\n", pid, cmd);
                    print(&line);
                }
            }
        }
        off += reclen;
    }
    skyos_libc::syscall::close(fd as u64);
    0
}
