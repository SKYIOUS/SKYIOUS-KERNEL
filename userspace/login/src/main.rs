#![no_std]
#![no_main]

extern crate alloc;

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

use alloc::string::String;
use alloc::vec::Vec;
use skyos_libc::syscall;

fn read_line() -> String {
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

fn read_passwd() -> String {
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

fn parse_passwd() -> Vec<(String, String, u64, u64, String, String, String)> {
    let data = read_file("/etc/passwd");
    let mut users = Vec::new();
    for line in data.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 7 {
            let uid: u64 = parts[2].parse().unwrap_or(0);
            let gid: u64 = parts[3].parse().unwrap_or(0);
            users.push((
                parts[0].into(), parts[1].into(), uid, gid,
                parts[4].into(), parts[5].into(), parts[6].into()
            ));
        }
    }
    users
}

fn parse_shadow() -> Vec<(String, String)> {
    let data = read_file("/etc/shadow");
    let mut entries = Vec::new();
    for line in data.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 2 {
            entries.push((parts[0].into(), parts[1].into()));
        }
    }
    entries
}

fn read_file(path: &str) -> String {
    let cpath = alloc::ffi::CString::new(path).ok();
    if cpath.is_none() { return String::new(); }
    let fd = syscall::open(cpath.unwrap().as_ptr() as *const u8, 0);
    if (fd as i64) < 0 { return String::new(); }
    let mut buf = [0u8; 4096];
    let n = syscall::read(fd, &mut buf);
    syscall::close(fd);
    if (n as i64) > 0 {
        String::from_utf8_lossy(&buf[..n as usize]).into_owned()
    } else {
        String::new()
    }
}

fn puts(s: &str) {
    syscall::write(1, s.as_bytes());
}

fn eputs(s: &str) {
    syscall::write(2, s.as_bytes());
}

fn simple_hash(pass: &str) -> String {
    let mut h: u32 = 5381;
    for b in pass.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u32);
    }
    alloc::format!("$sky${:08x}", h)
}

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    puts("SkyOS login: ");
    let username = read_line();
    let users = parse_passwd();
    let shadow = parse_shadow();
    let user = users.iter().find(|u| u.0 == username);
    let user = match user {
        Some(u) => u,
        None => {
            puts("\nLogin incorrect\n");
            return 1;
        }
    };
    let shadow_entry = shadow.iter().find(|s| s.0 == username);
    puts("Password: ");
    let password = read_passwd();
    puts("\n");
    let expected_hash = shadow_entry.map(|s| &s.1).unwrap_or(&user.1);
    if expected_hash == "*" || expected_hash == "!" {
        puts("Account locked\n");
        return 1;
    }
    if expected_hash != &"x" {
        if simple_hash(&password) != *expected_hash {
            puts("Login incorrect\n");
            return 1;
        }
    }
    puts(&alloc::format!("Welcome, {}!\n", username));
    let shell = &user.6;
    let cpath = alloc::ffi::CString::new(shell.as_str()).ok();
    if cpath.is_none() { eputs("No shell\n"); return 1; }
    let shell_ptr = cpath.unwrap().into_raw() as *const u8;
    let args = [shell_ptr, core::ptr::null()];
    unsafe {
        syscall::execve(args[0], args.as_ptr() as *const *const u8, core::ptr::null());
    }
    eputs("Failed to start shell\n");
    1
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
