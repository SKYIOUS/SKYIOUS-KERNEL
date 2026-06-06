use alloc::vec::Vec;
use alloc::string::String;

pub const AF_INET: u64 = 2;
pub const SOCK_DGRAM: u64 = 2;
pub const SOCK_STREAM: u64 = 1;
pub const SOCK_RAW: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Addr(pub [u8; 4]);

impl Ipv4Addr {
    pub fn new(a: u8, b: u8, c: u8, d: u8) -> Self { Ipv4Addr([a, b, c, d]) }
    pub fn from_u32(val: u32) -> Self {
        Ipv4Addr([(val >> 24) as u8, (val >> 16) as u8, (val >> 8) as u8, val as u8])
    }
    pub fn to_u32(&self) -> u32 { (self.0[0] as u32) << 24 | (self.0[1] as u32) << 16 | (self.0[2] as u32) << 8 | self.0[3] as u32 }
}

impl core::fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SocketAddrV4 {
    pub ip: Ipv4Addr,
    pub port: u16,
}

/// Resolve a hostname to an IPv4 address using the kernel's DNS resolver.
pub fn resolve(name: &str) -> Option<Ipv4Addr> {
    let cname = alloc::ffi::CString::new(name).ok()?;
    let mut ip_bytes = [0u8; 4];
    let ret = skyos_libc::syscall::syscall2(
        skyos_libc::SYS_RESOLVE,
        cname.as_ptr() as u64,
        ip_bytes.as_mut_ptr() as u64,
    );
    if ret == 0 { Some(Ipv4Addr(ip_bytes)) } else { None }
}

/// Create a socket. Returns fd or negative error.
pub fn socket(domain: u64, type_: u64, protocol: u64) -> i64 {
    let ret = skyos_libc::syscall::syscall3(skyos_libc::SYS_SOCKET, domain, type_, protocol);
    if (ret as i64) >= 0 { ret as i64 } else { -(ret as i64) }
}

/// Send a UDP datagram.
pub fn sendto(fd: i64, buf: &[u8], addr: &SocketAddrV4) -> i64 {
    let mut raw = [0u8; 8];
    raw[..2].copy_from_slice(&(AF_INET as u16).to_be_bytes());
    raw[2..4].copy_from_slice(&addr.port.to_be_bytes());
    raw[4..8].copy_from_slice(&addr.ip.0);
    let ret = skyos_libc::syscall::syscall5(
        skyos_libc::SYS_SENDTO, fd as u64, buf.as_ptr() as u64,
        buf.len() as u64, raw.as_ptr() as u64, 8,
    );
    if (ret as i64) >= 0 { ret as i64 } else { -(ret as i64) }
}

/// Receive a UDP datagram. Returns (bytes_received, source_addr).
pub fn recvfrom(fd: i64, buf: &mut [u8]) -> (i64, Option<SocketAddrV4>) {
    let mut raw = [0u8; 16];
    let mut addrlen: u32 = 16;
    let ret = skyos_libc::syscall::syscall5(
        skyos_libc::SYS_RECVFROM, fd as u64, buf.as_mut_ptr() as u64,
        buf.len() as u64, raw.as_mut_ptr() as u64, &mut addrlen as *mut u32 as u64,
    );
    if (ret as i64) < 0 { return (ret as i64, None); }
    let ip = Ipv4Addr([raw[4], raw[5], raw[6], raw[7]]);
    let port = u16::from_be_bytes([raw[2], raw[3]]);
    (ret as i64, Some(SocketAddrV4 { ip, port }))
}

/// Connect a socket to a remote address.
pub fn connect(fd: i64, addr: &SocketAddrV4) -> i64 {
    let mut raw = [0u8; 8];
    raw[..2].copy_from_slice(&(AF_INET as u16).to_be_bytes());
    raw[2..4].copy_from_slice(&addr.port.to_be_bytes());
    raw[4..8].copy_from_slice(&addr.ip.0);
    let ret = skyos_libc::syscall::syscall3(skyos_libc::SYS_CONNECT, fd as u64, raw.as_ptr() as u64, 8);
    if (ret as i64) >= 0 { 0 } else { -(ret as i64) }
}
