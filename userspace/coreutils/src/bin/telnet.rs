#![no_std]
#![no_main]
extern crate alloc;
use coreutils::{eprint, print};
use libskyos::net::{self, SocketAddrV4};
use alloc::vec::Vec;
#[global_allocator]
static A: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }
fn parse_host_port(s: &str) -> Option<SocketAddrV4> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 { return None; }
    let ip_parts: Vec<&str> = parts[0].split('.').collect();
    if ip_parts.len() != 4 { return None; }
    let mut octets = [0u8; 4];
    for (i, p) in ip_parts.iter().enumerate() {
        let mut val = 0u8;
        for &b in p.as_bytes() { if b >= b'0' && b <= b'9' { val = val * 10 + (b - b'0'); } }
        octets[i] = val;
    }
    let port = parts[1].bytes().fold(0u16, |acc, b| if b >= b'0' && b <= b'9' { acc * 10 + (b - b'0') as u16 } else { acc });
    Some(SocketAddrV4 { ip: libskyos::net::Ipv4Addr(octets), port })
}
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    if _argc < 2 { eprint("Usage: telnet <host> <port>\n"); return 1; }
    let host = unsafe { core::ffi::CStr::from_ptr(*_argv.add(1) as *const i8).to_str().unwrap_or("") };
    let port_str = if _argc > 2 {
        unsafe { core::ffi::CStr::from_ptr(*_argv.add(2) as *const i8).to_str().unwrap_or("23") }
    } else { "23" };
    let ip = if let Some(addr) = parse_host_port(&alloc::format!("{}:{}", host, port_str)) { addr }
        else if let Some(ip) = net::resolve(host) {
            let port = port_str.bytes().fold(0u16, |acc, b| if b >= b'0' && b <= b'9' { acc * 10 + (b - b'0') as u16 } else { acc });
            if port == 0 { SocketAddrV4 { ip, port: 23 } } else { SocketAddrV4 { ip, port } }
        } else { eprint("telnet: could not resolve "); eprint(host); eprint("\n"); return 1; };
    let msg = alloc::format!("telnet: connecting to {}:{}...\n", ip.ip, ip.port);
    print(&msg);
    let fd = net::socket(net::AF_INET, net::SOCK_STREAM, 0);
    if fd < 0 { eprint("telnet: socket failed\n"); return 1; }
    let ret = net::connect(fd, &ip);
    if ret < 0 { eprint("telnet: connect failed\n"); skyos_libc::syscall::close(fd as u64); return 1; }
    print("telnet: connected (type Ctrl+C to exit)\n");
    loop {
        let mut ch = [0u8; 1];
        let n = skyos_libc::syscall::read(0, &mut ch);
        if (n as i64) <= 0 || ch[0] == 3 { break; }
        skyos_libc::syscall::write(fd as u64, &ch);
        let mut reply = [0u8; 1];
        let r = skyos_libc::syscall::read(fd as u64, &mut reply);
        if (r as i64) > 0 { skyos_libc::syscall::write(1, &reply[..1]); }
    }
    skyos_libc::syscall::close(fd as u64);
    0
}
