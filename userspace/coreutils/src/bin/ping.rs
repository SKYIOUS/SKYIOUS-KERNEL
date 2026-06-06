#![no_std]
#![no_main]
extern crate alloc;
use coreutils::{eprint, print};
use libskyos::net::{self, Ipv4Addr, SocketAddrV4};
use alloc::vec::Vec;
#[global_allocator]
static A: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }
fn parse_ip(s: &str) -> Option<Ipv4Addr> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 { return None; }
    let mut octets = [0u8; 4];
    for (i, p) in parts.iter().enumerate() {
        match p.parse::<u8>() {
            Ok(v) => octets[i] = v,
            Err(_) => return None,
        }
    }
    Some(Ipv4Addr(octets))
}
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    if _argc < 2 { eprint("Usage: ping <host>\n"); return 1; }
    let host = unsafe { core::ffi::CStr::from_ptr(*_argv.add(1) as *const i8).to_str().unwrap_or("") };
    let ip = if let Some(ip) = parse_ip(host) { ip }
        else if let Some(ip) = net::resolve(host) { ip }
        else { eprint("ping: could not resolve "); eprint(host); eprint("\n"); return 1; };
    let fd = net::socket(net::AF_INET, net::SOCK_DGRAM, 0);
    if fd < 0 { eprint("ping: socket failed\n"); return 1; }
    let dest = SocketAddrV4 { ip, port: 0 };
    let payload = b"SkyOS ping";
    for seq in 0..4 {
        let msg = alloc::format!("ping {}: seq {} sending\n", ip, seq);
        print(&msg);
        let ret = net::sendto(fd, payload, &dest);
        if ret < 0 { eprint("ping: send failed\n"); break; }
        let mut recv_buf = [0u8; 64];
        let (n, _) = net::recvfrom(fd, &mut recv_buf);
        if n > 0 {
            let reply = core::str::from_utf8(&recv_buf[..n as usize]).unwrap_or("");
            let msg = alloc::format!("ping: seq {} reply {} bytes: {}\n", seq, n, reply);
            print(&msg);
        } else {
            let msg = alloc::format!("ping: seq {} timeout\n", seq);
            print(&msg);
        }
    }
    skyos_libc::syscall::close(fd as u64);
    0
}
