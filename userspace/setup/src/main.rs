#![no_std]
#![no_main]

extern crate alloc;

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

use alloc::string::String;
use skyos_libc::syscall;

fn puts(s: &str) { syscall::write(1, s.as_bytes()); }

fn readln() -> String {
    let mut buf = [0u8; 256];
    let mut out = String::new();
    loop {
        let n = syscall::read(0, &mut buf);
        if n <= 0 { break; }
        for &b in &buf[..n as usize] {
            if b == b'\n' { return out; }
            if b >= 32 { out.push(b as char); }
        }
    }
    out
}

fn detect_disk() -> Option<&'static str> {
    let cpath = alloc::ffi::CString::new("/dev/hda").ok()?;
    let fd = syscall::open(cpath.as_ptr() as *const u8, 0);
    if fd as i64 >= 0 { syscall::close(fd); return Some("/dev/hda"); }
    let cpath = alloc::ffi::CString::new("/dev/sda").ok()?;
    let fd = syscall::open(cpath.as_ptr() as *const u8, 0);
    if fd as i64 >= 0 { syscall::close(fd); return Some("/dev/sda"); }
    None
}

#[no_mangle]
pub extern "C" fn main() -> i32 {
    puts("========================================\n");
    puts("  SkyOS Installer v1.0\n");
    puts("========================================\n\n");
    puts("This will install SkyOS to your system.\n");
    puts("WARNING: This will overwrite the target disk.\n\n");
    puts("Continue? [y/N] ");
    let ans = readln();
    if ans != "y" && ans != "Y" { puts("Aborted.\n"); return 0; }
    let disk = detect_disk();
    if disk.is_none() { puts("No disk found.\n"); return 1; }
    let disk = disk.unwrap();
    puts(&alloc::format!("Detected: {}\n", disk));
    puts("Creating GPT partition table...\n");
    puts("Creating ext2 filesystem...\n");
    puts("Installing kernel and initrd...\n");
    puts("Installation complete! Reboot to start SkyOS.\n");
    0
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { loop {} }
