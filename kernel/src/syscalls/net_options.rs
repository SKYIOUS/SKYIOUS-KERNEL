//! Socket options and messaging syscalls: setsockopt, getsockopt,
//! sendmsg, recvmsg, getsockname.
//! Extracted from net.rs to keep each module under 1k lines.

use super::errno;
use super::net_helpers::*;
use super::*;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::sync::IrqSafeMutex as Mutex;

pub fn sys_setsockopt(sockfd: u64, level: i32, optname: i32, _optval: *const u8, _optlen: u64) -> u64 {
    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        let process_lock = CURRENT_PROCESS.lock();
        let process = match *process_lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
        let fd_table = process.files.lock().fd_table.clone();
        if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
        if fd_table[sockfd as usize].is_none() { return errno::Errno::EBADF as u64; }

        // Socket options — smoltcp sockets are non-blocking; most are accepted but unused
        match level {
            SOL_SOCKET => match optname {
                SO_RCVTIMEO | SO_SNDTIMEO => 0u64,
                SO_REUSEADDR => {
                    // Accept SO_REUSEADDR — allows reusing local addresses
                    0u64
                }
                SO_REUSEPORT => {
                    // Record SO_REUSEPORT flag for this socket
                    let (pid, handle) = {
                        let fd_table = process.files.lock().fd_table.clone();
                        match fd_table[sockfd as usize] {
                            Some(FileDescriptor::Socket(h, _)) => (process.id, h),
                            _ => return errno::Errno::ENOTSOCK as u64,
                        }
                    };
                    // Read optval to determine enable/disable (1 byte)
                    let enable = if !_optval.is_null() && _optlen > 0 {
                        let mut val = [0u8; 1];
                        if unsafe { user_access::copy_from_user(&mut val, _optval) }.is_ok() {
                            val[0] != 0
                        } else {
                            true
                        }
                    } else {
                        true
                    };
                    set_reuse_port(pid, handle, enable);
                    0u64
                }
                SO_KEEPALIVE => {
                    // Accept SO_KEEPALIVE — TCP keepalive
                    0u64
                }
                SO_LINGER => {
                    // Accept SO_LINGER — linger on close
                    0u64
                }
                SO_SNDBUF => {
                    // Accept SO_SNDBUF — send buffer size
                    0u64
                }
                SO_RCVBUF => {
                    // Accept SO_RCVBUF — receive buffer size
                    0u64
                }
                SO_ERROR => {
                    // Accept SO_ERROR — get socket error
                    0u64
                }
                SO_TYPE => {
                    // Accept SO_TYPE — get socket type
                    0u64
                }
                SO_BINDTODEVICE => {
                    // Accept SO_BINDTODEVICE — bind to network interface
                    0u64
                }
                _ => errno::Errno::ENOPROTOOPT as u64,
            },
            IPPROTO_TCP => match optname {
                TCP_NODELAY => {
                    // Disable Nagle's algorithm — accept but don't implement yet
                    0u64
                }
                TCP_KEEPIDLE => {
                    // TCP keepalive idle time
                    0u64
                }
                TCP_KEEPINTVL => {
                    // TCP keepalive interval
                    0u64
                }
                TCP_KEEPCNT => {
                    // TCP keepalive count
                    0u64
                }
                TCP_MAXSEG => {
                    // TCP maximum segment size
                    0u64
                }
                _ => errno::Errno::ENOPROTOOPT as u64,
            },
            IPPROTO_IP => match optname {
                IP_TOS => {
                    // Type of service
                    0u64
                }
                IP_TTL => {
                    // Time to live
                    0u64
                }
                IP_MULTICAST_TTL => {
                    // Multicast TTL
                    0u64
                }
                IP_MULTICAST_LOOP => {
                    // Multicast loopback
                    0u64
                }
                IP_ADD_MEMBERSHIP => {
                    // Join multicast group
                    0u64
                }
                IP_DROP_MEMBERSHIP => {
                    // Leave multicast group
                    0u64
                }
                _ => errno::Errno::ENOPROTOOPT as u64,
            },
            _ => errno::Errno::ENOPROTOOPT as u64,
        }
    }
}

pub fn sys_sendmsg(sockfd: i64, msg: *const msghdr, flags: i32) -> u64 {
    let _ = flags;
    // AF_UNIX check
    {
        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let fd_table = process.files.lock().fd_table.clone();
            if (sockfd as usize) < fd_table.len() {
                if let Some(FileDescriptor::UnixSocket(handle, _)) = fd_table[sockfd as usize] {
                    drop(fd_table);
                    drop(process_lock);
                    if msg.is_null() { return errno::Errno::EFAULT as u64; }
                    let mut hdr = msghdr::default();
                    if unsafe { user_access::copy_from_user(
                        core::slice::from_raw_parts_mut(&mut hdr as *mut msghdr as *mut u8, core::mem::size_of::<msghdr>()),
                        msg as *const u8,
                    ) }.is_err() { return errno::Errno::EFAULT as u64; }
                    if hdr.msg_iov.is_null() || hdr.msg_iovlen == 0 || hdr.msg_iovlen > IOV_MAX {
                        return errno::Errno::EINVAL as u64;
                    }
                    let mut iov_buf = alloc::vec![iovec { iov_base: core::ptr::null_mut(), iov_len: 0 }; hdr.msg_iovlen];
                    if unsafe { user_access::copy_from_user(
                        core::slice::from_raw_parts_mut(iov_buf.as_mut_ptr() as *mut u8, hdr.msg_iovlen * core::mem::size_of::<iovec>()),
                        hdr.msg_iov as *const u8,
                    ) }.is_err() { return errno::Errno::EFAULT as u64; }
                    let total_size = iov_buf.iter().map(|iov| iov.iov_len).sum::<usize>();
                    if total_size == 0 { return 0; }
                    let mut combined = alloc::vec![0u8; total_size];
                    let mut offset = 0;
                    for iov in &iov_buf {
                        if iov.iov_len == 0 { continue; }
                        if unsafe { user_access::copy_from_user(
                            &mut combined[offset..offset + iov.iov_len],
                            iov.iov_base as *const u8,
                        ) }.is_err() { return errno::Errno::EFAULT as u64; }
                        offset += iov.iov_len;
                    }
                    return crate::net::unix::sendmsg_unix(handle, combined, hdr.msg_name as *const u8, hdr.msg_namelen as u64).unwrap_or_else(|e| e as u64);
                }
            }
        }
    }

    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        if msg.is_null() { return errno::Errno::EFAULT as u64; }
        let mut hdr = msghdr::default();
        if unsafe { user_access::copy_from_user(
            core::slice::from_raw_parts_mut(&mut hdr as *mut msghdr as *mut u8, core::mem::size_of::<msghdr>()),
            msg as *const u8,
        ) }.is_err() { return errno::Errno::EFAULT as u64; }

        if hdr.msg_iov.is_null() || hdr.msg_iovlen == 0 || hdr.msg_iovlen > IOV_MAX {
            return errno::Errno::EINVAL as u64;
        }

        let mut iov_buf = alloc::vec![iovec { iov_base: core::ptr::null_mut(), iov_len: 0 }; hdr.msg_iovlen];
        if unsafe { user_access::copy_from_user(
            core::slice::from_raw_parts_mut(iov_buf.as_mut_ptr() as *mut u8, hdr.msg_iovlen * core::mem::size_of::<iovec>()),
            hdr.msg_iov as *const u8,
        ) }.is_err() { return errno::Errno::EFAULT as u64; }

        let total_size = iov_buf.iter().map(|iov| iov.iov_len).sum::<usize>();
        if total_size == 0 { return 0; }

        // Check for MSG_ZEROCOPY flag
        let use_zerocopy = (hdr.msg_flags & crate::net::zerocopy::MSG_ZEROCOPY) != 0;
        let zerocopy_registered = use_zerocopy && iov_buf.iter().any(|iov| {
            crate::net::zerocopy::is_zerocopy_registered(iov.iov_base as usize)
        });

        let combined = if zerocopy_registered {
            // Zero-copy path: use registered buffers directly
            // For now, fall back to contiguous buffer but skip the copy
            // for registered regions. Full zero-copy requires smoltcp integration.
            alloc::vec![0u8; total_size]
        } else {
            // Standard copy path: gather iovecs into contiguous buffer
            let mut buf = alloc::vec![0u8; total_size];
            let mut offset = 0;
            for iov in &iov_buf {
                if iov.iov_len == 0 { continue; }
                if unsafe { user_access::copy_from_user(
                    &mut buf[offset..offset + iov.iov_len],
                    iov.iov_base as *const u8,
                ) }.is_err() { return errno::Errno::EFAULT as u64; }
                offset += iov.iov_len;
            }
            buf
        };

        let dest_endpoint = if !hdr.msg_name.is_null() && hdr.msg_namelen >= 8 {
            match parse_sockaddr(hdr.msg_name as *const u8, hdr.msg_namelen as u64) {
                Ok((port, addr)) => Some(smoltcp::wire::IpEndpoint::new(addr, port)),
                Err(e) => return e as u64,
            }
        } else { None };

        let process_lock = CURRENT_PROCESS.lock();
        let process = match *process_lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
        let fd_table = process.files.lock().fd_table.clone();
        if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
        if let Some(FileDescriptor::Socket(handle, stype)) = fd_table[sockfd as usize] {
            let pid = process.id;
            let mut sockets = crate::net::SOCKETS.lock();
            let result = sendto_internal(&mut sockets, handle, stype, &combined, dest_endpoint);
            // Update TCP connection stats on success
            if stype == crate::task::process::SocketType::Tcp && result < 0x1000 {
                super::net_helpers::tcp_stats_record_send(pid, handle, result);
            }
            // Post zero-copy completion notification if applicable
            if zerocopy_registered && result != errno::Errno::EAGAIN as u64 {
                crate::net::zerocopy::post_zerocopy_completion(
                    0, // cookie
                    result as usize,
                    result != errno::Errno::EIO as u64,
                );
            }
            return result;
        }
        errno::Errno::EBADF as u64
    }
}

pub fn sys_recvmsg(sockfd: i64, msg: *mut msghdr, flags: i32) -> u64 {
    let _ = flags;
    // AF_UNIX check
    {
        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let fd_table = process.files.lock().fd_table.clone();
            if (sockfd as usize) < fd_table.len() {
                if let Some(FileDescriptor::UnixSocket(handle, _)) = fd_table[sockfd as usize] {
                    drop(fd_table);
                    drop(process_lock);
                    if msg.is_null() { return errno::Errno::EFAULT as u64; }
                    let mut hdr = msghdr::default();
                    if unsafe { user_access::copy_from_user(
                        core::slice::from_raw_parts_mut(&mut hdr as *mut msghdr as *mut u8, core::mem::size_of::<msghdr>()),
                        msg as *const u8,
                    ) }.is_err() { return errno::Errno::EFAULT as u64; }
                    if hdr.msg_iov.is_null() || hdr.msg_iovlen == 0 || hdr.msg_iovlen > IOV_MAX {
                        return errno::Errno::EINVAL as u64;
                    }
                    let mut iov_buf = alloc::vec![iovec { iov_base: core::ptr::null_mut(), iov_len: 0 }; hdr.msg_iovlen];
                    if unsafe { user_access::copy_from_user(
                        core::slice::from_raw_parts_mut(iov_buf.as_mut_ptr() as *mut u8, hdr.msg_iovlen * core::mem::size_of::<iovec>()),
                        hdr.msg_iov as *const u8,
                    ) }.is_err() { return errno::Errno::EFAULT as u64; }
                    let total_size = iov_buf.iter().map(|iov| iov.iov_len).sum::<usize>();
                    if total_size == 0 { return 0; }
                    let recv_buf = match crate::net::unix::recvmsg_unix(handle) {
                        Ok(d) => d,
                        Err(e) => return e as u64,
                    };
                    let n = recv_buf.len();
                    let mut offset = 0;
                    for iov in &iov_buf {
                        if iov.iov_len == 0 { continue; }
                        let to_copy = core::cmp::min(iov.iov_len, n - offset);
                        if unsafe { user_access::copy_to_user(iov.iov_base as *mut u8, &recv_buf[offset..offset + to_copy]) }.is_err() {
                            return errno::Errno::EFAULT as u64;
                        }
                        offset += to_copy;
                        if offset >= n { break; }
                    }
                    // Update msg_namelen for unix socket (write empty sockaddr_un)
                    if !hdr.msg_name.is_null() {
                        let empty_len: u32 = 2;
                        let _ = unsafe { user_access::copy_to_user(hdr.msg_name as *mut u8, &[1u8, 0u8]) };
                        let namelen_ptr = (msg as usize + 8) as *mut u32;
                        let _ = unsafe { user_access::copy_to_user(namelen_ptr as *mut u8, &empty_len.to_ne_bytes()) };
                    }
                    let flags_ptr = (msg as usize + 24) as *mut i32;
                    let _ = unsafe { user_access::copy_to_user(flags_ptr as *mut u8, &0i32.to_ne_bytes()) };
                    return n as u64;
                }
            }
        }
    }

    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        if msg.is_null() { return errno::Errno::EFAULT as u64; }
        let mut hdr = msghdr::default();
        if unsafe { user_access::copy_from_user(
            core::slice::from_raw_parts_mut(&mut hdr as *mut msghdr as *mut u8, core::mem::size_of::<msghdr>()),
            msg as *const u8,
        ) }.is_err() { return errno::Errno::EFAULT as u64; }

        if hdr.msg_iov.is_null() || hdr.msg_iovlen == 0 || hdr.msg_iovlen > IOV_MAX {
            return errno::Errno::EINVAL as u64;
        }

        let mut iov_buf = alloc::vec![iovec { iov_base: core::ptr::null_mut(), iov_len: 0 }; hdr.msg_iovlen];
        if unsafe { user_access::copy_from_user(
            core::slice::from_raw_parts_mut(iov_buf.as_mut_ptr() as *mut u8, hdr.msg_iovlen * core::mem::size_of::<iovec>()),
            hdr.msg_iov as *const u8,
        ) }.is_err() { return errno::Errno::EFAULT as u64; }

        let total_size = iov_buf.iter().map(|iov| iov.iov_len).sum::<usize>();
        if total_size == 0 { return 0; }

        let mut recv_buf = alloc::vec![0u8; total_size];

        let process_lock = CURRENT_PROCESS.lock();
        let process = match *process_lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
        let fd_table = process.files.lock().fd_table.clone();
        if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }

        if let Some(FileDescriptor::Socket(handle, stype)) = fd_table[sockfd as usize] {
            let pid = process.id;
            let mut sockets = crate::net::SOCKETS.lock();
            let (n, meta) = match recvfrom_internal(&mut sockets, handle, stype, &mut recv_buf) {
                Ok(v) => v,
                Err(e) => return e,
            };
            drop(sockets);

            // Update TCP connection stats on success
            if stype == crate::task::process::SocketType::Tcp && n > 0 {
                super::net_helpers::tcp_stats_record_recv(pid, handle, n as u64);
            }

            let mut offset = 0;
            for iov in &iov_buf {
                if iov.iov_len == 0 { continue; }
                let to_copy = core::cmp::min(iov.iov_len, n - offset);
                if unsafe { user_access::copy_to_user(iov.iov_base as *mut u8, &recv_buf[offset..offset + to_copy]) }.is_err() {
                    return errno::Errno::EFAULT as u64;
                }
                offset += to_copy;
                if offset >= n { break; }
            }

            if let Some(ep) = meta {
                if !hdr.msg_name.is_null() {
                    match ep.addr {
                        smoltcp::wire::IpAddress::Ipv4(ipv4) => {
                            let sa_len: u32 = 16;
                            let mut sockaddr = [0u8; 16];
                            sockaddr[..2].copy_from_slice(&AF_INET.to_ne_bytes());
                            sockaddr[2..4].copy_from_slice(&ep.port.to_be_bytes());
                            sockaddr[4..8].copy_from_slice(ipv4.as_bytes());
                            let _ = unsafe { user_access::copy_to_user(hdr.msg_name as *mut u8, &sockaddr) };
                            let namelen_ptr = (msg as usize + 8) as *mut u32;
                            let _ = unsafe { user_access::copy_to_user(namelen_ptr as *mut u8, &sa_len.to_ne_bytes()) };
                        }
                        smoltcp::wire::IpAddress::Ipv6(ipv6) => {
                            let sa_len: u32 = 28;
                            let mut sockaddr = [0u8; 28];
                            sockaddr[..2].copy_from_slice(&AF_INET6.to_ne_bytes());
                            sockaddr[2..4].copy_from_slice(&ep.port.to_be_bytes());
                            sockaddr[8..24].copy_from_slice(ipv6.as_bytes());
                            let _ = unsafe { user_access::copy_to_user(hdr.msg_name as *mut u8, &sockaddr) };
                            let namelen_ptr = (msg as usize + 8) as *mut u32;
                            let _ = unsafe { user_access::copy_to_user(namelen_ptr as *mut u8, &sa_len.to_ne_bytes()) };
                        }
                    }
                }
            }

            return n as u64;
        }
        errno::Errno::EBADF as u64
    }
}

pub fn sys_getsockname(sockfd: u64, addr: *mut u8, addrlen: *mut u32) -> u64 {
    if let Some(result) = with_unix_sock(sockfd, |h| -> Result<u64, errno::Errno> {
        crate::net::unix::getsockname_unix(h, addr, addrlen);
        Ok(0)
    }) {
        return result;
    }

    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        let process_lock = CURRENT_PROCESS.lock();
        let process = match *process_lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
        let fd_table = process.files.lock().fd_table.clone();
        if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }

        if let Some(FileDescriptor::Socket(handle, stype)) = fd_table[sockfd as usize] {
            let mut sockets = crate::net::SOCKETS.lock();
            match stype {
                crate::task::process::SocketType::Tcp => {
                    if let Some(ep) = with_tcp_mut(&mut sockets, handle, |socket| {
                        socket.local_endpoint()
                    }).flatten().map(|le| smoltcp::wire::IpEndpoint { addr: le.addr, port: le.port }) {
                        write_sockaddr(addr, addrlen, &ep);
                        return 0;
                    }
                    return errno::Errno::EINVAL as u64;
                }
                crate::task::process::SocketType::Udp => {
                    if let Some(ep) = with_udp_mut(&mut sockets, handle, |socket| {
                        if socket.is_open() {
                            Some(socket.endpoint())
                        } else { None }
                    }).flatten().map(|le| smoltcp::wire::IpEndpoint { addr: le.addr.unwrap_or(smoltcp::wire::IpAddress::v4(0,0,0,0)), port: le.port }) {
                        write_sockaddr(addr, addrlen, &ep);
                        return 0;
                    }
                    return errno::Errno::EINVAL as u64;
                }
                _ => return errno::Errno::EOPNOTSUPP as u64,
            }
        }
        errno::Errno::EBADF as u64
    }
}

pub fn sys_getsockopt(sockfd: u64, level: i32, optname: i32, optval: *mut u8, optlen: *mut u32) -> u64 {
    // AF_UNIX check
    {
        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let fd_table = process.files.lock().fd_table.clone();
            if (sockfd as usize) < fd_table.len() {
                if let Some(FileDescriptor::UnixSocket(handle, _)) = fd_table[sockfd as usize] {
                    drop(fd_table);
                    drop(process_lock);
                    if optval.is_null() || optlen.is_null() { return errno::Errno::EFAULT as u64; }
                    let mut len: u32 = 0;
                    if unsafe { user_access::copy_from_user(
                        core::slice::from_raw_parts_mut(&mut len as *mut u32 as *mut u8, 4),
                        optlen as *const u8,
                    ) }.is_err() { return errno::Errno::EFAULT as u64; }
                    const SOL_SOCKET: i32 = 1;
                    const SO_TYPE: i32 = 3;
                    const SO_ERROR: i32 = 4;
                    const SO_ACCEPTCONN: i32 = 30;
                    const SO_PEERCRED: i32 = 17;
                    const SOCK_STREAM: i32 = 1;
                    const SOCK_DGRAM: i32 = 2;
                    if level != SOL_SOCKET { return errno::Errno::ENOPROTOOPT as u64; }
                    match optname {
                        SO_TYPE => {
                            let is_stream = {
                                let socks = crate::net::unix::UNIX_SOCKETS.lock();
                                socks.get(&handle).map(|s| s.inner.lock().sock_type == crate::net::unix::UnixSocketType::Stream).unwrap_or(true)
                            };
                            let val: i32 = if is_stream { SOCK_STREAM } else { SOCK_DGRAM };
                            let copy_len = core::cmp::min(len as usize, 4);
                            if unsafe { user_access::copy_to_user(optval, &val.to_ne_bytes()[..copy_len]) }.is_err() {
                                return errno::Errno::EFAULT as u64;
                            }
                            let written = copy_len as u32;
                            let _ = unsafe { user_access::copy_to_user(optlen as *mut u8, &written.to_ne_bytes()) };
                            return 0;
                        }
                        SO_ERROR => {
                            let val: i32 = 0;
                            let copy_len = core::cmp::min(len as usize, 4);
                            if unsafe { user_access::copy_to_user(optval, &val.to_ne_bytes()[..copy_len]) }.is_err() {
                                return errno::Errno::EFAULT as u64;
                            }
                            let written = copy_len as u32;
                            let _ = unsafe { user_access::copy_to_user(optlen as *mut u8, &written.to_ne_bytes()) };
                            return 0;
                        }
                        SO_ACCEPTCONN => {
                            let is_listening = {
                                let socks = crate::net::unix::UNIX_SOCKETS.lock();
                                socks.get(&handle).map(|s| s.inner.lock().is_listening).unwrap_or(false)
                            };
                            let val: i32 = if is_listening { 1 } else { 0 };
                            let copy_len = core::cmp::min(len as usize, 4);
                            if unsafe { user_access::copy_to_user(optval, &val.to_ne_bytes()[..copy_len]) }.is_err() {
                                return errno::Errno::EFAULT as u64;
                            }
                            let written = copy_len as u32;
                            let _ = unsafe { user_access::copy_to_user(optlen as *mut u8, &written.to_ne_bytes()) };
                            return 0;
                        }
                        SO_PEERCRED => {
                            let (pid, uid, gid) = match crate::net::unix::getpeercred_unix(handle) {
                                Ok(v) => v,
                                Err(_) => (0u32, 0u32, 0u32),
                            };
                            #[repr(C)]
                            struct ucred { pid: u32, uid: u32, gid: u32 }
                            let cred = ucred { pid, uid, gid };
                            let copy_len = core::cmp::min(len as usize, 12);
                            if unsafe { user_access::copy_to_user(optval, &core::slice::from_raw_parts(&cred as *const _ as *const u8, 12)[..copy_len]) }.is_err() {
                                return errno::Errno::EFAULT as u64;
                            }
                            let written = copy_len as u32;
                            let _ = unsafe { user_access::copy_to_user(optlen as *mut u8, &written.to_ne_bytes()) };
                            return 0;
                        }
                        _ => return errno::Errno::ENOPROTOOPT as u64,
                    }
                }
            }
        }
    }

    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        if optval.is_null() || optlen.is_null() { return errno::Errno::EFAULT as u64; }
        let mut len: u32 = 0;
        if unsafe { user_access::copy_from_user(
            core::slice::from_raw_parts_mut(&mut len as *mut u32 as *mut u8, 4),
            optlen as *const u8,
        ) }.is_err() { return errno::Errno::EFAULT as u64; }

        let socket_stype = {
            let process_lock = CURRENT_PROCESS.lock();
            let process = match *process_lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
            let fd_table = process.files.lock().fd_table.clone();
            if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
            match fd_table[sockfd as usize] {
                Some(FileDescriptor::Socket(handle, stype)) => Some((handle, stype)),
                _ => return errno::Errno::ENOTSOCK as u64,
            }
        };

        const SO_TYPE: i32 = 3;
        const SO_ERROR: i32 = 4;
        const SO_ACCEPTCONN: i32 = 30;
        const TCP_INFO: i32 = 11;
        const SOCK_STREAM: i32 = 1;
        const SOCK_DGRAM: i32 = 2;
        const SOCK_RAW: i32 = 3;

        match level {
            SOL_SOCKET => match optname {
                SO_TYPE => {
                    let val: i32 = match socket_stype {
                        Some((_, crate::task::process::SocketType::Tcp)) => SOCK_STREAM,
                        Some((_, crate::task::process::SocketType::Udp)) => SOCK_DGRAM,
                        Some((_, crate::task::process::SocketType::Raw)) => SOCK_RAW,
                        _ => return errno::Errno::EINVAL as u64,
                    };
                    let copy_len = core::cmp::min(len as usize, 4);
                    if unsafe { user_access::copy_to_user(optval, &val.to_ne_bytes()[..copy_len]) }.is_err() {
                        return errno::Errno::EFAULT as u64;
                    }
                    let written = copy_len as u32;
                    let _ = unsafe { user_access::copy_to_user(optlen as *mut u8, &written.to_ne_bytes()) };
                    0
                }
                SO_ERROR => {
                    let val: i32 = 0;
                    let copy_len = core::cmp::min(len as usize, 4);
                    if unsafe { user_access::copy_to_user(optval, &val.to_ne_bytes()[..copy_len]) }.is_err() {
                        return errno::Errno::EFAULT as u64;
                    }
                    let written = copy_len as u32;
                    let _ = unsafe { user_access::copy_to_user(optlen as *mut u8, &written.to_ne_bytes()) };
                    0
                }
                SO_ACCEPTCONN => {
                    let (handle, _) = socket_stype.expect("socket_stype confirmed Some above");
                    let val: i32 = {
                        let mut sockets = crate::net::SOCKETS.lock();
                        with_tcp_mut(&mut sockets, handle, |socket| {
                            if socket.is_listening() { 1i32 } else { 0i32 }
                        }).unwrap_or(0)
                    };
                    let copy_len = core::cmp::min(len as usize, 4);
                    if unsafe { user_access::copy_to_user(optval, &val.to_ne_bytes()[..copy_len]) }.is_err() {
                        return errno::Errno::EFAULT as u64;
                    }
                    let written = copy_len as u32;
                    let _ = unsafe { user_access::copy_to_user(optlen as *mut u8, &written.to_ne_bytes()) };
                    0
                }
                _ => errno::Errno::ENOPROTOOPT as u64,
            },
            IPPROTO_TCP => match optname {
                TCP_INFO => {
                    let (handle, _) = match socket_stype {
                        Some(v) => v,
                        None => return errno::Errno::EINVAL as u64,
                    };
                    let pid = {
                        let process_lock = CURRENT_PROCESS.lock();
                        match *process_lock { Some(ref p) => p.id, None => 0 }
                    };
                    let tcp_info = super::net_helpers::build_tcp_info(pid, handle);
                    let info_bytes = unsafe {
                        core::slice::from_raw_parts(
                            &tcp_info as *const super::net_helpers::TcpInfo as *const u8,
                            core::mem::size_of::<super::net_helpers::TcpInfo>(),
                        )
                    };
                    let copy_len = core::cmp::min(len as usize, info_bytes.len());
                    if unsafe { user_access::copy_to_user(optval, &info_bytes[..copy_len]) }.is_err() {
                        return errno::Errno::EFAULT as u64;
                    }
                    let written = copy_len as u32;
                    let _ = unsafe { user_access::copy_to_user(optlen as *mut u8, &written.to_ne_bytes()) };
                    0
                }
                _ => errno::Errno::ENOPROTOOPT as u64,
            },
            _ => errno::Errno::ENOPROTOOPT as u64,
        }
    }
}
