#![no_std]
#![no_main]

extern crate alloc;

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

use alloc::string::String;
use alloc::vec::Vec;
use skyos_libc::syscall;

fn read_file(path: &str) -> String {
    let cpath = alloc::ffi::CString::new(path).ok();
    if cpath.is_none() { return String::new(); }
    let fd = syscall::open(cpath.unwrap().as_ptr() as *const u8, 0);
    if (fd as i64) < 0 { return String::new(); }
    let mut buf = [0u8; 4096];
    let n = syscall::read(fd, &mut buf);
    syscall::close(fd);
    if (n as i64) > 0 { String::from_utf8_lossy(&buf[..n as usize]).into_owned() } else { String::new() }
}

fn write_file(path: &str, data: &str) -> bool {
    let cpath = alloc::ffi::CString::new(path).ok();
    if cpath.is_none() { return false; }
    let fd = syscall::open(cpath.unwrap().as_ptr() as *const u8, 0x42);
    if (fd as i64) < 0 { return false; }
    let written = syscall::write(fd, data.as_bytes());
    syscall::close(fd);
    (written as i64) >= 0
}

fn puts(s: &str) { syscall::write(1, s.as_bytes()); }
fn eputs(s: &str) { syscall::write(2, s.as_bytes()); }

fn read_pass() -> String {
    let mut s = String::new();
    let mut buf = [0u8; 1];
    loop {
        let n = syscall::read(0, &mut buf);
        if (n as i64) <= 0 { break; }
        if buf[0] == b'\n' { break; }
        if buf[0] == b'\r' { continue; }
        s.push(buf[0] as char);
    }
    s
}

fn simple_hash(pass: &str) -> String {
    let mut h: u32 = 5381;
    for b in pass.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u32);
    }
    alloc::format!("$sky${:08x}", h)
}

#[no_mangle]
pub extern "C" fn main(_argc: u64, argv: *const *const u8) -> i32 {
    let mut new_user = false;
    if !argv.is_null() {
        let ptr = unsafe { argv.offset(1) };
        if !unsafe { *ptr }.is_null() {
            let arg = unsafe { core::ffi::CStr::from_ptr(*ptr as *const i8) }.to_str().unwrap_or("");
            if !arg.is_empty() { new_user = true; }
        }
    }
    puts("Current password: ");
    let _old = read_pass();
    puts("\nNew password: ");
    let p1 = read_pass();
    puts("\nConfirm: ");
    let p2 = read_pass();
    puts("\n");
    if p1 != p2 {
        eputs("passwd: passwords do not match\n");
        return 1;
    }
    if p1.len() < 3 {
        eputs("passwd: password too short\n");
        return 1;
    }
    let hash = simple_hash(&p1);
    let shadow = read_file("/etc/shadow");
    let mut new_shadow = String::new();
    let mut found = false;
    for line in shadow.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.is_empty() { continue; }
        if parts[0] == "root" || (new_user && parts[0] == "root") {
            new_shadow.push_str(&alloc::format!("{}:{}:18000:0:99999:7:::\n", parts[0], hash));
            found = true;
        } else {
            new_shadow.push_str(line);
            new_shadow.push('\n');
        }
    }
    if !found {
        new_shadow.push_str(&alloc::format!("root:{}:18000:0:99999:7:::\n", hash));
    }
    write_file("/etc/shadow", &new_shadow);
    puts("Password updated.\n");
    0
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
