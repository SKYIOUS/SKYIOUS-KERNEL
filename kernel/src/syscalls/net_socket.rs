//! Socket lifecycle syscalls: socket, bind, connect, listen, accept,
//! sendto, recvfrom, socketpair.
//! Extracted from net.rs to keep each module under 1k lines.

use super::errno;
use super::net_helpers::*;
use super::*;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::sync::IrqSafeMutex as Mutex;
use alloc::vec;

pub fn sys_socket(domain: u64, ty: u64, _protocol: u64) -> u64 {
    // AF_UNIX (domain 1) — always available, no net feature gate needed
    if domain == 1 {
        if ty != 1 && ty != 2 {
            return errno::Errno::EINVAL as u64;
        }
        let handle = match crate::net::unix::create_unix_socket(ty) {
            Ok(h) => h,
            Err(e) => return e as u64,
        };
        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let mut fd_table = process.files.lock().fd_table.clone();
            let fd_obj = FileDescriptor::UnixSocket(handle, crate::task::process::SocketType::Unix);
            for (i, slot) in fd_table.iter_mut().enumerate() {
                if slot.is_none() {
                    *slot = Some(fd_obj);
                    return i as u64;
                }
            }
            fd_table.push(Some(fd_obj));
            return (fd_table.len() - 1) as u64;
        }
        return errno::Errno::ESRCH as u64;
    }

    if domain != AF_INET as u64 && domain != AF_INET6 as u64 {
        return errno::Errno::EAFNOSUPPORT as u64;
    }
    // LSM: socket creation permission
    {
        let subj = crate::security::current_subject();
        if !crate::security::hook_socket_create(&subj, domain) {
            return errno::Errno::EACCES as u64;
        }
    }

    #[cfg(not(feature = "net"))]
    {
        let _ = ty;
        return errno::Errno::ENOSYS as u64;
    }

    #[cfg(feature = "net")]
    {
        // SOCK_RAW requires CAP_NET_RAW
        if ty == 3 && !has_capability(CAP_NET_RAW) {
            audit_log("CAP_NET_RAW", "socket(SOCK_RAW) DENIED");
            return errno::Errno::EPERM as u64;
        }

        let handle = if ty == 2 { // SOCK_DGRAM
            let rx_buffer = smoltcp::socket::udp::PacketBuffer::new(vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 16], vec![0; 4096]);
            let tx_buffer = smoltcp::socket::udp::PacketBuffer::new(vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 16], vec![0; 4096]);
            let socket = smoltcp::socket::udp::Socket::new(rx_buffer, tx_buffer);
            crate::net::SOCKETS.lock().add(socket)
        } else if ty == 1 { // SOCK_STREAM
            let rx_buffer = smoltcp::socket::tcp::SocketBuffer::new(vec![0; 4096]);
            let tx_buffer = smoltcp::socket::tcp::SocketBuffer::new(vec![0; 4096]);
            let socket = smoltcp::socket::tcp::Socket::new(rx_buffer, tx_buffer);
            crate::net::SOCKETS.lock().add(socket)
        } else if ty == 3 { // SOCK_RAW — backed by ICMP socket for ping
            let rx_buffer = smoltcp::socket::udp::PacketBuffer::new(
                vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 4],
                vec![0u8; 4096],
            );
            let tx_buffer = smoltcp::socket::udp::PacketBuffer::new(
                vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 4],
                vec![0u8; 4096],
            );
            let socket = smoltcp::socket::udp::Socket::new(rx_buffer, tx_buffer);
            crate::net::SOCKETS.lock().add(socket)
        } else {
            return errno::Errno::EINVAL as u64;
        };

        let socket_type = if ty == 1 { crate::task::process::SocketType::Tcp }
            else if ty == 3 { crate::task::process::SocketType::Raw }
            else { crate::task::process::SocketType::Udp };
        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let mut fd_table = process.files.lock().fd_table.clone();
            let fd_obj = FileDescriptor::Socket(handle, socket_type);
            for (i, slot) in fd_table.iter_mut().enumerate() {
                if slot.is_none() {
                    *slot = Some(fd_obj);
                    return i as u64;
                }
            }
            fd_table.push(Some(fd_obj));
            return (fd_table.len() - 1) as u64;
        }
        errno::Errno::ESRCH as u64
    }
}

pub fn sys_bind(sockfd: u64, addr_ptr: *const u8, addrlen: u64) -> u64 {
    if let Some(result) = with_unix_sock(sockfd, |h| crate::net::unix::bind_unix(h, addr_ptr, addrlen).map(|_| 0)) {
        return result;
    }

    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        let (_port, addr) = match parse_sockaddr(addr_ptr, addrlen) {
            Ok(v) => v,
            Err(e) => return e as u64,
        };
        let endpoint = smoltcp::wire::IpEndpoint::new(addr, _port);

        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let fd_table = process.files.lock().fd_table.clone();
            if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
            if let Some(FileDescriptor::Socket(handle, stype)) = fd_table[sockfd as usize] {
                let pid = process.id;
                match stype {
                    crate::task::process::SocketType::Udp => {
                        let mut sockets = crate::net::SOCKETS.lock();
                        let success = with_udp_mut(&mut sockets, handle, |socket| {
                            socket.bind(endpoint).is_ok()
                        }).unwrap_or(false);
                        if !success { return errno::Errno::EADDRINUSE as u64; }
                    }
                    crate::task::process::SocketType::Tcp => {
                        TCP_BIND_ENDPOINTS.lock().insert((pid, handle), endpoint);
                    }
                    crate::task::process::SocketType::Raw |
                    crate::task::process::SocketType::Unix => {
                        // raw/unix sockets handled above before net feature gate
                    }
                }
                return 0;
            }
        }
        errno::Errno::EBADF as u64
    }
}

pub fn sys_connect(sockfd: u64, addr_ptr: *const u8, addrlen: u64) -> u64 {
    if let Some(result) = with_unix_sock(sockfd, |h| crate::net::unix::connect_unix(h, addr_ptr, addrlen).map(|_| 0)) {
        return result;
    }

    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        let (_port, addr) = match parse_sockaddr(addr_ptr, addrlen) {
            Ok(v) => v,
            Err(e) => return e as u64,
        };
        let endpoint = smoltcp::wire::IpEndpoint::new(addr, _port);

        // LSM: socket connect permission
        let subj = crate::security::current_subject();
        let addr_str = alloc::format!("{}", endpoint);
        if !crate::security::hook_socket_connect(&subj, &addr_str) {
            return errno::Errno::EACCES as u64;
        }

        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let fd_table = process.files.lock().fd_table.clone();
            if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
            if let Some(FileDescriptor::Socket(handle, stype)) = fd_table[sockfd as usize] {
                let mut sockets = crate::net::SOCKETS.lock();
                match stype {
                    crate::task::process::SocketType::Tcp => {
                        let mut iface_lock = crate::net::NETWORK_INTERFACE.lock();
                        let result = iface_lock.as_mut().map(|iface| {
                            let cx = iface.context();
                            with_tcp_mut(&mut sockets, handle, |socket| {
                                if !socket.is_active() {
                                    let local_endpoint = smoltcp::wire::IpListenEndpoint {
                                        addr: None,
                                        port: 0,
                                    };
                                    if socket.connect(cx, endpoint, local_endpoint).is_err() {
                                        Err(errno::Errno::ECONNREFUSED)
                                    } else {
                                        Ok(0u64)
                                    }
                                } else if socket.may_send() {
                                    Ok(0u64)
                                } else {
                                    Err(errno::Errno::EALREADY)
                                }
                            })
                        });
                        match result {
                            Some(Some(Ok(v))) => return v,
                            Some(Some(Err(e))) => return e as u64,
                            _ => return errno::Errno::EIO as u64,
                        }
                    }
                    crate::task::process::SocketType::Udp => {
                        return 0;
                    }
                    crate::task::process::SocketType::Raw |
                    crate::task::process::SocketType::Unix => {
                        // raw/unix sockets are connectionless
                        return 0;
                    }
                }
            }
        }
        errno::Errno::EBADF as u64
    }
}

pub fn sys_listen(sockfd: u64, _backlog: u64) -> u64 {
    if let Some(result) = with_unix_sock(sockfd, |h| crate::net::unix::listen_unix(h).map(|_| 0)) {
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
            if stype != crate::task::process::SocketType::Tcp {
                return errno::Errno::EOPNOTSUPP as u64;
            }
            let pid = process.id;
            let bind_ep = TCP_BIND_ENDPOINTS.lock().get(&(pid, handle)).copied();
            let port = bind_ep.map(|ep| ep.port).unwrap_or(0);
            if port == 0 { return errno::Errno::EINVAL as u64; }
            let mut sockets = crate::net::SOCKETS.lock();
            let success = with_tcp_mut(&mut sockets, handle, |socket| {
                let listen_ep = smoltcp::wire::IpListenEndpoint {
                    addr: None,
                    port,
                };
                socket.listen(listen_ep).is_ok()
            }).unwrap_or(false);
            if !success { return errno::Errno::EADDRINUSE as u64; }
            return 0;
        }
        errno::Errno::EBADF as u64
    }
}

pub fn sys_accept(sockfd: u64, addr_ptr: *mut u8, addrlen_ptr: *mut u32) -> u64 {
    // AF_UNIX check
    {
        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let fd_table = process.files.lock().fd_table.clone();
            if (sockfd as usize) < fd_table.len() {
                if let Some(FileDescriptor::UnixSocket(handle, _)) = fd_table[sockfd as usize] {
                    drop(fd_table);
                    let new_handle = match crate::net::unix::accept_unix(handle, addr_ptr, addrlen_ptr) {
                        Ok(h) => h,
                        Err(e) => return e as u64,
                    };
                    let fd_obj = FileDescriptor::UnixSocket(new_handle, crate::task::process::SocketType::Unix);
                    let process_lock2 = CURRENT_PROCESS.lock();
                    if let Some(ref process2) = *process_lock2 {
                        let mut fd_table2 = process2.files.lock().fd_table.clone();
                        for (i, slot) in fd_table2.iter_mut().enumerate() {
                            if slot.is_none() {
                                *slot = Some(fd_obj);
                                return i as u64;
                            }
                        }
                        fd_table2.push(Some(fd_obj));
                        return (fd_table2.len() - 1) as u64;
                    }
                }
            }
        }
    }

    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        if check_signal_interrupt() { return errno::Errno::EINTR as u64; }
        crate::net::poll();

        let process = {
            let process_lock = CURRENT_PROCESS.lock();
            match *process_lock { Some(ref p) => p.clone(), None => return errno::Errno::ESRCH as u64 }
        };
        let mut fd_table = process.files.lock().fd_table.clone();
        if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }

        let (handle, local_port) = match fd_table[sockfd as usize] {
            Some(FileDescriptor::Socket(h, stype)) => {
                if stype != crate::task::process::SocketType::Tcp {
                    return errno::Errno::EOPNOTSUPP as u64;
                }
                let mut sockets = crate::net::SOCKETS.lock();
                let result = with_tcp_mut(&mut sockets, h, |socket| {
                    if socket.is_listening() || !socket.is_open() {
                        return Err(errno::Errno::EAGAIN);
                    }
                    let remote = socket.remote_endpoint();
                    let local_port = socket.local_endpoint().map(|ep| ep.port).unwrap_or(0);
                    Ok((remote, local_port))
                });
                match result {
                    Some(Ok((Some(ep), lp))) => {
                        write_sockaddr(addr_ptr, addrlen_ptr, &ep);
                        (h, lp)
                    }
                    Some(Ok((None, _))) => return errno::Errno::EINVAL as u64,
                    Some(Err(e)) => return e as u64,
                    None => return errno::Errno::EINVAL as u64,
                }
            }
            _ => return errno::Errno::EBADF as u64,
        };

        if local_port == 0 { return errno::Errno::EINVAL as u64; }

        let rx_buffer = smoltcp::socket::tcp::SocketBuffer::new(vec![0u8; 4096]);
        let tx_buffer = smoltcp::socket::tcp::SocketBuffer::new(vec![0u8; 4096]);
        let mut new_socket = smoltcp::socket::tcp::Socket::new(rx_buffer, tx_buffer);
        let listen_addr = smoltcp::wire::IpListenEndpoint {
            addr: None,
            port: local_port,
        };
        if new_socket.listen(listen_addr).is_err() {
            return errno::Errno::EADDRINUSE as u64;
        }

        let mut sockets = crate::net::SOCKETS.lock();
        let new_handle = sockets.add(new_socket);

        let pid = process.id;
        if let Some(ep) = TCP_BIND_ENDPOINTS.lock().get(&(pid, handle)).copied() {
            TCP_BIND_ENDPOINTS.lock().insert((pid, new_handle), ep);
        }

        for (i, slot) in fd_table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(FileDescriptor::Socket(new_handle, crate::task::process::SocketType::Tcp));
                return i as u64;
            }
        }
        fd_table.push(Some(FileDescriptor::Socket(new_handle, crate::task::process::SocketType::Tcp)));
        (fd_table.len() - 1) as u64
    }
}

pub fn sys_sendto(sockfd: u64, buf: *const u8, len: u64, addr_ptr: *const u8, addrlen: u64) -> u64 {
    if let Some(result) = with_unix_sock(sockfd, |h| crate::net::unix::sendto_unix(h, buf, len, addr_ptr, addrlen)) {
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
            let mut data = vec![0u8; len as usize];
            if unsafe { user_access::copy_from_user(&mut data, buf) }.is_err() { return errno::Errno::EFAULT as u64; }

            let dest_endpoint = if !addr_ptr.is_null() && addrlen >= 8 {
                match parse_sockaddr(addr_ptr, addrlen) {
                    Ok((port, addr)) => Some(smoltcp::wire::IpEndpoint::new(addr, port)),
                    Err(_) => return errno::Errno::EINVAL as u64,
                }
            } else {
                 None
            };

            let mut sockets = crate::net::SOCKETS.lock();
            return sendto_internal(&mut sockets, handle, stype, &data, dest_endpoint);
        }
        errno::Errno::EBADF as u64
    }
}

#[cfg(not(feature = "net"))]
pub fn sys_recvfrom(sockfd: u64, buf: *mut u8, len: u64, addr_ptr: *mut u8, addrlen_ptr: *mut u32) -> u64 {
    if let Some(result) = with_unix_sock(sockfd, |h| crate::net::unix::recvfrom_unix(h, buf, len, addr_ptr, addrlen_ptr)) {
        return result;
    }
    errno::Errno::ENOSYS as u64
}

#[cfg(feature = "net")]
pub fn sys_recvfrom(sockfd: u64, buf: *mut u8, len: u64, addr_ptr: *mut u8, addrlen_ptr: *mut u32) -> u64 {
    if let Some(result) = with_unix_sock(sockfd, |h| crate::net::unix::recvfrom_unix(h, buf, len, addr_ptr, addrlen_ptr)) {
        return result;
    }

    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
    let fd_table = process.files.lock().fd_table.clone();
    if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }

    if let Some(FileDescriptor::Socket(handle, stype)) = fd_table[sockfd as usize] {
        let mut sockets = crate::net::SOCKETS.lock();
        let mut data = vec![0u8; len as usize];
        match recvfrom_internal(&mut sockets, handle, stype, &mut data) {
            Ok((n, meta)) => {
                if let Some(ep) = meta {
                    write_sockaddr(addr_ptr, addrlen_ptr, &ep);
                }
                if unsafe { user_access::copy_to_user(buf, &data[..n]) }.is_ok() {
                    return n as u64;
                }
                return errno::Errno::EFAULT as u64;
            }
            Err(e) => return e,
        }
    }
    errno::Errno::EBADF as u64
}

pub fn sys_socketpair(domain: u64, type_: u64, protocol: u64, sv: *mut i32) -> u64 {
    crate::net::unix::sys_socketpair(domain, type_, protocol, sv).unwrap_or_else(|e| e as u64)
}
