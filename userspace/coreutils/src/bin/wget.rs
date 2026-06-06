#![no_std]
#![no_main]
extern crate alloc;
use coreutils::{eprint, print};
use libskyos::net::{self, SocketAddrV4};
#[global_allocator]
static A: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    if _argc < 2 { eprint("Usage: wget <url>\n"); return 1; }
    let url = unsafe { core::ffi::CStr::from_ptr(*_argv.add(1) as *const i8).to_str().unwrap_or("") };
    let url_str = url.strip_prefix("http://").unwrap_or(url);
    let (host, path) = if let Some(pos) = url_str.find('/') {
        (&url_str[..pos], &url_str[pos..])
    } else { (url_str, "/") };
    let ip = match net::resolve(host) {
        Some(ip) => ip,
        None => { eprint("wget: could not resolve "); eprint(host); eprint("\n"); return 1; }
    };
    let msg = alloc::format!("wget: resolved {} -> {}\n", host, ip);
    print(&msg);
    let msg = alloc::format!("wget: connecting to {}:80{}\n", ip, path);
    print(&msg);
    let fd = net::socket(net::AF_INET, net::SOCK_STREAM, 0);
    if fd < 0 { eprint("wget: socket failed\n"); return 1; }
    let dest = SocketAddrV4 { ip, port: 80 };
    let ret = net::connect(fd, &dest);
    if ret < 0 {
        let msg = alloc::format!("wget: connect failed (errno: {})\n", -ret);
        print(&msg);
        skyos_libc::syscall::close(fd as u64);
        return 1;
    }
    let request = alloc::format!("GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n", path, host);
    let sent = net::sendto(fd, request.as_bytes(), &SocketAddrV4 { ip, port: 80 });
    if sent < 0 { eprint("wget: send failed\n"); skyos_libc::syscall::close(fd as u64); return 1; }
    let mut buf = [0u8; 4096];
    loop {
        let n = skyos_libc::syscall::read(fd as u64, &mut buf);
        if (n as i64) <= 0 { break; }
        print(core::str::from_utf8(&buf[..n as usize]).unwrap_or(""));
    }
    skyos_libc::syscall::close(fd as u64);
    0
}
