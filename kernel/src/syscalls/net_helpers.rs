//! Network internal helpers: constants, types, statics, and utility functions.
//! Extracted from net.rs to keep each module under 1k lines.

use super::errno;
use crate::sync::IrqSafeMutex as Mutex;
use hashbrown::HashMap;
use smoltcp::socket::{Socket, tcp, udp};
use smoltcp::wire::IpEndpoint;
use smoltcp::iface::SocketHandle;

// ─── Address family constants ─────────────────────────────────────

pub(crate) const AF_INET: u16 = 2;
pub(crate) const AF_INET6: u16 = 10;
pub(crate) const MAX_SOCK_ADDR_LEN: u64 = 128;
pub(crate) const IOV_MAX: usize = 1024;

// ─── Socket option constants ──────────────────────────────────────

pub(crate) const SOL_SOCKET: i32 = 1;
pub(crate) const SO_RCVTIMEO: i32 = 20;
pub(crate) const SO_SNDTIMEO: i32 = 21;
pub(crate) const SO_REUSEADDR: i32 = 2;
pub(crate) const SO_REUSEPORT: i32 = 15;
pub(crate) const SO_KEEPALIVE: i32 = 9;
pub(crate) const SO_LINGER: i32 = 13;
pub(crate) const SO_SNDBUF: i32 = 7;
pub(crate) const SO_RCVBUF: i32 = 8;
pub(crate) const SO_ERROR: i32 = 4;
pub(crate) const SO_TYPE: i32 = 3;
pub(crate) const SO_BINDTODEVICE: i32 = 25;
pub(crate) const IPPROTO_TCP: i32 = 6;
pub(crate) const TCP_NODELAY: i32 = 1;
pub(crate) const TCP_KEEPIDLE: i32 = 4;
pub(crate) const TCP_KEEPINTVL: i32 = 5;
pub(crate) const TCP_KEEPCNT: i32 = 6;
pub(crate) const TCP_MAXSEG: i32 = 2;
pub(crate) const IPPROTO_IP: i32 = 0;
pub(crate) const IP_TOS: i32 = 1;
pub(crate) const IP_TTL: i32 = 2;
pub(crate) const IP_MULTICAST_TTL: i32 = 33;
pub(crate) const IP_MULTICAST_LOOP: i32 = 34;
pub(crate) const IP_ADD_MEMBERSHIP: i32 = 35;
pub(crate) const IP_DROP_MEMBERSHIP: i32 = 36;

// ─── C types for sendmsg/recvmsg ──────────────────────────────────

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct iovec {
    pub iov_base: *mut u8,
    pub iov_len: usize,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct msghdr {
    pub msg_name: *mut u8,
    pub msg_namelen: u32,
    pub msg_iov: *const iovec,
    pub msg_iovlen: usize,
    pub msg_control: *mut u8,
    pub msg_controllen: usize,
    pub msg_flags: i32,
}

// ─── Statics ──────────────────────────────────────────────────────

lazy_static::lazy_static! {
    pub(crate) static ref TCP_BIND_ENDPOINTS: Mutex<HashMap<(u64, SocketHandle), IpEndpoint>> =
        Mutex::new(HashMap::new());
}

// ─── Socket access helpers ────────────────────────────────────────

/// Safely access a TCP socket by handle without panicking on type mismatch.
pub(crate) fn with_tcp_mut<R>(sockets: &mut smoltcp::iface::SocketSet, handle: smoltcp::iface::SocketHandle, f: impl FnOnce(&mut tcp::Socket) -> R) -> Option<R> {
    for (h, socket) in sockets.iter_mut() {
        if h == handle {
            if let Socket::Tcp(ref mut s) = socket {
                return Some(f(s));
            }
            return None;
        }
    }
    None
}

/// Safely access a UDP socket by handle without panicking on type mismatch.
pub(crate) fn with_udp_mut<R>(sockets: &mut smoltcp::iface::SocketSet, handle: smoltcp::iface::SocketHandle, f: impl FnOnce(&mut udp::Socket) -> R) -> Option<R> {
    for (h, socket) in sockets.iter_mut() {
        if h == handle {
            if let Socket::Udp(ref mut s) = socket {
                return Some(f(s));
            }
            return None;
        }
    }
    None
}

// ─── Internal send/recv ───────────────────────────────────────────

/// Internal: send data on a socket given handle+type. Returns bytes sent or errno.
pub(crate) fn sendto_internal(
    sockets: &mut smoltcp::iface::SocketSet,
    handle: SocketHandle,
    stype: crate::task::process::SocketType,
    data: &[u8],
    dest_endpoint: Option<smoltcp::wire::IpEndpoint>,
) -> u64 {
    match stype {
        crate::task::process::SocketType::Udp => {
            if let Some(endpoint) = dest_endpoint {
                if with_udp_mut(sockets, handle, |socket| {
                    socket.send_slice(data, endpoint).is_ok()
                }).unwrap_or(false) {
                    return data.len() as u64;
                }
            }
        }
        crate::task::process::SocketType::Tcp => {
            if with_tcp_mut(sockets, handle, |socket| {
                if socket.may_send() {
                    let result = socket.send(|slice| {
                        let n = core::cmp::min(slice.len(), data.len());
                        slice[..n].copy_from_slice(&data[..n]);
                        (n, true)
                    });
                    if result.unwrap_or(false) {
                        return data.len() as u64;
                    }
                }
                errno::Errno::EAGAIN as u64
            }).is_some() {
                return data.len() as u64;
            }
        }
        _ => return errno::Errno::ENOSYS as u64,
    }
    errno::Errno::EIO as u64
}

/// Internal: receive data from a socket into a kernel buffer.
/// Returns (bytes_received, endpoint) on success.
pub(crate) fn recvfrom_internal(
    sockets: &mut smoltcp::iface::SocketSet,
    handle: SocketHandle,
    stype: crate::task::process::SocketType,
    buf: &mut [u8],
) -> Result<(usize, Option<smoltcp::wire::IpEndpoint>), u64> {
    match stype {
        crate::task::process::SocketType::Tcp => {
            let mut result = Err(errno::Errno::EAGAIN as u64);
            if let Some(_) = with_tcp_mut(sockets, handle, |socket| {
                if socket.may_recv() {
                    match socket.recv_slice(buf) {
                        Ok(n) => {
                            #[cfg(feature = "ash")]
                            {
                                let src = socket.remote_endpoint().map(|e| e.port).unwrap_or(0);
                                let dst = socket.local_endpoint().map(|e| e.port).unwrap_or(0);
                                if crate::ash::hooks::net::hook_net_receive(&mut buf[..n], 0, 6, src, dst)
                                    == crate::ash::AshResult::Drop
                                {
                                    result = Err(errno::Errno::EAGAIN as u64);
                                    return;
                                }
                            }
                            result = Ok((n, None));
                        }
                        Err(_) => {}
                    }
                }
            }) { result.map_err(|_| errno::Errno::EAGAIN as u64) } else { Err(errno::Errno::EINVAL as u64) }
        }
        crate::task::process::SocketType::Udp => {
            let mut result = Err(errno::Errno::EAGAIN as u64);
            if let Some(_) = with_udp_mut(sockets, handle, |socket| {
                if let Ok((n, meta)) = socket.recv_slice(buf) {
                    #[cfg(feature = "ash")]
                    {
                        let src = meta.endpoint.port;
                        let dst = socket.endpoint().port;
                        if crate::ash::hooks::net::hook_net_receive(&mut buf[..n], 0, 17, src, dst)
                            == crate::ash::AshResult::Drop
                        {
                            result = Err(errno::Errno::EAGAIN as u64);
                            return;
                        }
                    }
                    result = Ok((n, Some(meta.endpoint)));
                }
            }) { result.map_err(|_| errno::Errno::EAGAIN as u64) } else { Err(errno::Errno::EINVAL as u64) }
        }
        _ => Err(errno::Errno::ENOSYS as u64),
    }
}

// ─── Sockaddr helpers ─────────────────────────────────────────────

pub(crate) fn parse_sockaddr(addr_ptr: *const u8, addrlen: u64) -> Result<(u16, smoltcp::wire::IpAddress), errno::Errno> {
    if addr_ptr.is_null() || addrlen < 8 || addrlen > MAX_SOCK_ADDR_LEN {
        return Err(errno::Errno::EINVAL);
    }
    let mut family_buf = [0u8; 2];
    unsafe { super::user_access::copy_from_user(&mut family_buf, addr_ptr).map_err(|_| errno::Errno::EFAULT)?; }
    let family = u16::from_ne_bytes(family_buf);
    if addrlen < (if family == AF_INET6 { 28 } else { 16 }) {
        return Err(errno::Errno::EINVAL);
    }
    let mut port_buf = [0u8; 2];
    unsafe { super::user_access::copy_from_user(&mut port_buf, addr_ptr.wrapping_add(2)).map_err(|_| errno::Errno::EFAULT)?; }
    let port = u16::from_be_bytes(port_buf);
    Ok((port,
        match family {
            AF_INET => {
                let mut ip = [0u8; 4];
                unsafe { super::user_access::copy_from_user(&mut ip, addr_ptr.wrapping_add(4)).map_err(|_| errno::Errno::EFAULT)?; }
                smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::from_bytes(&ip))
            }
            AF_INET6 => {
                let mut ip = [0u8; 16];
                unsafe { super::user_access::copy_from_user(&mut ip, addr_ptr.wrapping_add(8)).map_err(|_| errno::Errno::EFAULT)?; }
                smoltcp::wire::IpAddress::Ipv6(smoltcp::wire::Ipv6Address::from_bytes(&ip))
            }
            _ => return Err(errno::Errno::EAFNOSUPPORT),
        }))
}

/// Execute a closure if the fd is a Unix socket, returning the closure's result as u64.
/// Returns None if the fd is not a Unix socket (caller should fall through to TCP/UDP path).
pub(crate) fn with_unix_sock<F, E>(sockfd: u64, f: F) -> Option<u64>
where
    F: FnOnce(u64) -> Result<u64, E>,
    E: Into<u64>,
{
    let process_lock = crate::task::process::CURRENT_PROCESS.lock();
    if let Some(ref process) = *process_lock {
        let fd_table = process.fd_table.lock();
        if (sockfd as usize) < fd_table.len() {
            if let Some(crate::task::process::FileDescriptor::UnixSocket(handle, _)) = fd_table[sockfd as usize] {
                return Some(f(handle).unwrap_or_else(|e| e.into()));
            }
        }
    }
    None
}

pub(crate) fn write_sockaddr(addr_ptr: *mut u8, addrlen_ptr: *mut u32, ep: &smoltcp::wire::IpEndpoint) {
    if addr_ptr.is_null() || addrlen_ptr.is_null() { return; }
    match ep.addr {
        smoltcp::wire::IpAddress::Ipv4(ipv4) => {
            let mut sockaddr = [0u8; 16];
            sockaddr[..2].copy_from_slice(&AF_INET.to_ne_bytes());
            sockaddr[2..4].copy_from_slice(&ep.port.to_be_bytes());
            sockaddr[4..8].copy_from_slice(ipv4.as_bytes());
            let addr_len: u32 = 16;
            let _ = unsafe { super::user_access::copy_to_user(addr_ptr, &sockaddr) };
            let _ = unsafe { super::user_access::copy_to_user(addrlen_ptr as *mut u8, &addr_len.to_ne_bytes()) };
        }
        smoltcp::wire::IpAddress::Ipv6(ipv6) => {
            let mut sockaddr = [0u8; 28];
            sockaddr[..2].copy_from_slice(&AF_INET6.to_ne_bytes());
            sockaddr[2..4].copy_from_slice(&ep.port.to_be_bytes());
            sockaddr[8..24].copy_from_slice(ipv6.as_bytes());
            let addr_len: u32 = 28;
            let _ = unsafe { super::user_access::copy_to_user(addr_ptr, &sockaddr) };
            let _ = unsafe { super::user_access::copy_to_user(addrlen_ptr as *mut u8, &addr_len.to_ne_bytes()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sockaddr_null_ptr() {
        let result = parse_sockaddr(core::ptr::null(), 16);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_sockaddr_too_short() {
        let data = [0u8; 4];
        let result = parse_sockaddr(data.as_ptr(), 4);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_sockaddr_too_long() {
        let data = [0u8; 200];
        let result = parse_sockaddr(data.as_ptr(), 200);
        assert!(result.is_err());
    }
}
