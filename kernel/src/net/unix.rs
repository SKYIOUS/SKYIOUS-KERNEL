extern crate alloc;
use alloc::collections::VecDeque;
use hashbrown::HashMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use crate::sync::IrqSafeMutex as Mutex;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;

use crate::syscalls::errno::Errno;
use crate::syscalls::user_access;

const AF_UNIX: u16 = 1;
const SOCK_STREAM: u64 = 1;
const SOCK_DGRAM: u64 = 2;

#[derive(Clone, Copy, PartialEq)]
pub enum UnixSocketType {
    Stream,
    Dgram,
}

#[derive(Clone)]
pub struct UnixSocket {
    pub inner: Arc<Mutex<UnixSocketInner>>,
}

pub struct UnixSocketInner {
    pub bind_path: Option<String>,
    pub peer: Option<u64>,
    pub is_listening: bool,
    pub backlog: Vec<u64>,
    pub recv_queue: VecDeque<Vec<u8>>,
    pub closed: bool,
    pub sock_type: UnixSocketType,
}

lazy_static! {
    pub static ref UNIX_BOUND: Mutex<HashMap<String, u64>> = Mutex::new(HashMap::new());
    pub static ref UNIX_SOCKETS: Mutex<HashMap<u64, UnixSocket>> = Mutex::new(HashMap::new());
}
pub static NEXT_UNIX_HANDLE: AtomicU64 = AtomicU64::new(1000);

// Lock ordering: UNIX_BOUND -> UNIX_SOCKETS -> individual socket inner.
// Never hold UNIX_SOCKETS while acquiring UNIX_BOUND.

fn alloc_handle() -> u64 {
    NEXT_UNIX_HANDLE.fetch_add(1, Ordering::Relaxed)
}

fn read_sockaddr_un(addr_ptr: *const u8, addrlen: u64) -> Result<String, Errno> {
    if addr_ptr.is_null() || addrlen < 2 {
        return Err(Errno::EINVAL);
    }
    let mut buf = [0u8; 110];
    let read_len = core::cmp::min(addrlen as usize, buf.len());
    unsafe {
        user_access::copy_from_user(&mut buf[..read_len], addr_ptr).map_err(|_| Errno::EFAULT)?;
    }
    let family = u16::from_ne_bytes([buf[0], buf[1]]);
    if family != AF_UNIX {
        return Err(Errno::EAFNOSUPPORT);
    }
    let path_bytes = &buf[2..read_len];
    let nul_pos = path_bytes.iter().position(|&b| b == 0).unwrap_or(path_bytes.len());
    let path = core::str::from_utf8(&path_bytes[..nul_pos]).map_err(|_| Errno::EINVAL)?;
    if path.is_empty() {
        return Err(Errno::EINVAL);
    }
    Ok(String::from(path))
}

pub fn write_sockaddr_un(addr_ptr: *mut u8, addrlen_ptr: *mut u32, path: &str) {
    if addr_ptr.is_null() || addrlen_ptr.is_null() {
        return;
    }
    let path_bytes = path.as_bytes();
    let total_len = 2 + core::cmp::min(path_bytes.len(), 108usize);
    let mut buf = vec![0u8; total_len];
    buf[..2].copy_from_slice(&AF_UNIX.to_ne_bytes());
    buf[2..total_len].copy_from_slice(&path_bytes[..total_len - 2]);
    let len32 = total_len as u32;
    unsafe {
        let _ = user_access::copy_to_user(addr_ptr, &buf);
        let _ = user_access::copy_to_user(addrlen_ptr as *mut u8, &len32.to_ne_bytes());
    }
}

pub fn create_unix_socket(type_: u64) -> Result<u64, Errno> {
    let sock_type = match type_ {
        SOCK_STREAM => UnixSocketType::Stream,
        SOCK_DGRAM => UnixSocketType::Dgram,
        _ => return Err(Errno::EINVAL),
    };
    let handle = alloc_handle();
    let sock = UnixSocket {
        inner: Arc::new(Mutex::new(UnixSocketInner {
            bind_path: None,
            peer: None,
            is_listening: false,
            backlog: Vec::new(),
            recv_queue: VecDeque::new(),
            closed: false,
            sock_type,
        })),
    };
    UNIX_SOCKETS.lock().insert(handle, sock);
    Ok(handle)
}

pub fn bind_unix(handle: u64, addr_ptr: *const u8, addrlen: u64) -> Result<(), Errno> {
    let path = read_sockaddr_un(addr_ptr, addrlen)?;
    let mut bound = UNIX_BOUND.lock();
    if bound.contains_key(&path) {
        return Err(Errno::EADDRINUSE);
    }
    let socks = UNIX_SOCKETS.lock();
    let sock = socks.get(&handle).ok_or(Errno::EBADF)?;
    sock.inner.lock().bind_path = Some(path.clone());
    bound.insert(path, handle);
    Ok(())
}

pub fn connect_unix(handle: u64, addr_ptr: *const u8, addrlen: u64) -> Result<(), Errno> {
    let path = read_sockaddr_un(addr_ptr, addrlen)?;
    let target_handle = {
        let bound = UNIX_BOUND.lock();
        bound.get(&path).copied().ok_or(Errno::ECONNREFUSED)?
    };
    let sock_type;
    let target_listening;
    {
        let socks = UNIX_SOCKETS.lock();
        let sock = socks.get(&handle).ok_or(Errno::EBADF)?;
        let target = socks.get(&target_handle).ok_or(Errno::ECONNREFUSED)?;
        sock_type = sock.inner.lock().sock_type;
        target_listening = target.inner.lock().is_listening;
    }
    if sock_type == UnixSocketType::Stream && !target_listening {
        return Err(Errno::ECONNREFUSED);
    }
    {
        let socks = UNIX_SOCKETS.lock();
        let target = socks.get(&target_handle).ok_or(Errno::ECONNREFUSED)?;
        if sock_type == UnixSocketType::Stream {
            target.inner.lock().backlog.push(handle);
        }
        let sock = socks.get(&handle).ok_or(Errno::EBADF)?;
        sock.inner.lock().peer = Some(target_handle);
    }
    if sock_type == UnixSocketType::Stream {
        crate::task::scheduler::wake_pipe(target_handle);
    }
    Ok(())
}

pub fn listen_unix(handle: u64) -> Result<(), Errno> {
    let socks = UNIX_SOCKETS.lock();
    let sock = socks.get(&handle).ok_or(Errno::EBADF)?;
    let mut inner = sock.inner.lock();
    if inner.bind_path.is_none() {
        return Err(Errno::EINVAL);
    }
    inner.is_listening = true;
    Ok(())
}

pub fn accept_unix(handle: u64, addr_ptr: *mut u8, addrlen_ptr: *mut u32) -> Result<u64, Errno> {
    let mut client_handle = 0u64;
    let mut bind_path = None;
    loop {
        if crate::syscalls::check_signal_interrupt() {
            return Err(Errno::EINTR);
        }
        let found = {
            let socks = UNIX_SOCKETS.lock();
            let sock = socks.get(&handle).ok_or(Errno::EBADF)?;
            let mut inner = sock.inner.lock();
            if !inner.is_listening {
                return Err(Errno::EINVAL);
            }
            if let Some(ch) = inner.backlog.first().copied() {
                inner.backlog.remove(0);
                client_handle = ch;
                bind_path = inner.bind_path.clone();
                true
            } else {
                false
            }
        };
        if found {
            break;
        }
        crate::task::scheduler::block_on_pipe(handle);
    }
    let new_handle = alloc_handle();
    let new_sock = UnixSocket {
        inner: Arc::new(Mutex::new(UnixSocketInner {
            bind_path: bind_path.clone(),
            peer: Some(client_handle),
            is_listening: false,
            backlog: Vec::new(),
            recv_queue: VecDeque::new(),
            closed: false,
            sock_type: UnixSocketType::Stream,
        })),
    };
    {
        let socks = UNIX_SOCKETS.lock();
        if let Some(client_sock) = socks.get(&client_handle) {
            client_sock.inner.lock().peer = Some(new_handle);
        }
    }
    if let Some(ref path) = bind_path {
        write_sockaddr_un(addr_ptr, addrlen_ptr, path);
    }
    UNIX_SOCKETS.lock().insert(new_handle, new_sock);
    Ok(new_handle)
}

pub fn sendto_unix(handle: u64, buf: *const u8, len: u64, addr_ptr: *const u8, addrlen: u64) -> Result<u64, Errno> {
    // Cap single message to avoid OOM panic from a hostile len.
    const MAX_UNIX_MSG: u64 = 1 << 20;
    let len = core::cmp::min(len, MAX_UNIX_MSG);
    let mut data = vec![0u8; len as usize];
    unsafe {
        user_access::copy_from_user(&mut data, buf).map_err(|_| Errno::EFAULT)?;
    }
    let peer_handle;
    let is_dgram_with_addr;
    {
        let socks = UNIX_SOCKETS.lock();
        let sock = socks.get(&handle).ok_or(Errno::EBADF)?;
        let inner = sock.inner.lock();
        if inner.closed {
            return Err(Errno::EPIPE);
        }
        is_dgram_with_addr = inner.sock_type == UnixSocketType::Dgram
            && !addr_ptr.is_null() && addrlen >= 2;
        peer_handle = inner.peer;
    }
    // DGRAM with explicit address: look up from path
    let target = if is_dgram_with_addr {
        if let Ok(path) = read_sockaddr_un(addr_ptr, addrlen) {
            let bound = UNIX_BOUND.lock();
            bound.get(&path).copied()
        } else {
            None
        }
    } else {
        None
    };
    let target_handle = target.or(peer_handle).ok_or(Errno::EDESTADDRREQ)?;
    let socks = UNIX_SOCKETS.lock();
    let peer = socks.get(&target_handle).ok_or(Errno::ECONNRESET)?;
    let mut peer_inner = peer.inner.lock();
    if peer_inner.closed {
        return Err(Errno::EPIPE);
    }
    peer_inner.recv_queue.push_back(data);
    drop(peer_inner);
    crate::task::scheduler::wake_pipe(target_handle);
    Ok(len)
}

pub fn recvfrom_unix(handle: u64, buf: *mut u8, len: u64, _addr_ptr: *mut u8, _addrlen_ptr: *mut u32) -> Result<u64, Errno> {
    let mut data = Vec::new();
    loop {
        if crate::syscalls::check_signal_interrupt() {
            return Err(Errno::EINTR);
        }
        let mut got_data = false;
        {
            let socks = UNIX_SOCKETS.lock();
            let sock = socks.get(&handle).ok_or(Errno::EBADF)?;
            let mut inner = sock.inner.lock();
            if let Some(d) = inner.recv_queue.pop_front() {
                data = d;
                got_data = true;
            } else if inner.closed {
                return Ok(0);
            }
        }
        if got_data {
            break;
        }
        crate::task::scheduler::block_on_pipe(handle);
    }
    let copy_len = core::cmp::min(data.len(), len as usize);
    unsafe {
        user_access::copy_to_user(buf, &data[..copy_len]).map_err(|_| Errno::EFAULT)?;
    }
    Ok(copy_len as u64)
}

pub fn sendmsg_unix(handle: u64, data: Vec<u8>, addr_ptr: *const u8, addrlen: u64) -> Result<u64, Errno> {
    let peer_handle;
    let is_dgram_with_addr;
    {
        let socks = UNIX_SOCKETS.lock();
        let sock = socks.get(&handle).ok_or(Errno::EBADF)?;
        let inner = sock.inner.lock();
        if inner.closed { return Err(Errno::EPIPE); }
        is_dgram_with_addr = inner.sock_type == UnixSocketType::Dgram && !addr_ptr.is_null() && addrlen >= 2;
        peer_handle = inner.peer;
    }
    let target = if is_dgram_with_addr {
        if let Ok(path) = read_sockaddr_un(addr_ptr, addrlen) {
            let bound = UNIX_BOUND.lock();
            bound.get(&path).copied()
        } else { None }
    } else { None };
    let target_handle = target.or(peer_handle).ok_or(Errno::EDESTADDRREQ)?;
    let socks = UNIX_SOCKETS.lock();
    let peer = socks.get(&target_handle).ok_or(Errno::ECONNRESET)?;
    let mut peer_inner = peer.inner.lock();
    if peer_inner.closed { return Err(Errno::EPIPE); }
    let len = data.len() as u64;
    peer_inner.recv_queue.push_back(data);
    drop(peer_inner);
    crate::task::scheduler::wake_pipe(target_handle);
    Ok(len)
}

pub fn recvmsg_unix(handle: u64) -> Result<Vec<u8>, Errno> {
    loop {
        if crate::syscalls::check_signal_interrupt() {
            return Err(Errno::EINTR);
        }
        let mut data = Vec::new();
        let got_data = {
            let socks = UNIX_SOCKETS.lock();
            let sock = socks.get(&handle).ok_or(Errno::EBADF)?;
            let mut inner = sock.inner.lock();
            if let Some(d) = inner.recv_queue.pop_front() {
                data = d;
                true
            } else if inner.closed {
                true
            } else {
                false
            }
        };
        if got_data {
            return Ok(data);
        }
        crate::task::scheduler::block_on_pipe(handle);
    }
}

pub fn getsockname_unix(handle: u64, addr_ptr: *mut u8, addrlen_ptr: *mut u32) {
    let socks = UNIX_SOCKETS.lock();
    if let Some(sock) = socks.get(&handle) {
        let inner = sock.inner.lock();
        if let Some(path) = &inner.bind_path {
            write_sockaddr_un(addr_ptr, addrlen_ptr, path);
        } else {
            write_sockaddr_un(addr_ptr, addrlen_ptr, "");
        }
    }
}

pub fn getpeername_unix(handle: u64, addr_ptr: *mut u8, addrlen_ptr: *mut u32) -> Result<(), Errno> {
    let peer_handle = {
        let socks = UNIX_SOCKETS.lock();
        let sock = socks.get(&handle).ok_or(Errno::EBADF)?;
        let peer = sock.inner.lock().peer.ok_or(Errno::ENOTCONN)?;
        peer
    };
    let socks = UNIX_SOCKETS.lock();
    let peer = socks.get(&peer_handle).ok_or(Errno::ECONNRESET)?;
    let inner = peer.inner.lock();
    if let Some(path) = &inner.bind_path {
        write_sockaddr_un(addr_ptr, addrlen_ptr, path);
    } else {
        write_sockaddr_un(addr_ptr, addrlen_ptr, "");
    }
    Ok(())
}

pub fn getpeercred_unix(handle: u64) -> Result<(u32, u32, u32), Errno> {
    let _sock = {
        let socks = UNIX_SOCKETS.lock();
        socks.get(&handle).ok_or(Errno::EBADF)?.clone()
    };
    let lock = crate::task::process::CURRENT_PROCESS.lock();
    let pid = lock.as_ref().map(|p| p.id as u32).unwrap_or(0);
    let (uid, gid) = lock.as_ref().map(|p| {
        let c = p.creds.lock();
        (c.uid, c.gid)
    }).unwrap_or((0, 0));
    Ok((pid, uid, gid))
}

/// Reports whether the peer socket has queued data or is closed. Used by
/// sys_poll so POLLIN is only asserted when a non-blocking read will return.
pub fn socket_has_data(handle: u64) -> bool {
    let socks = UNIX_SOCKETS.lock();
    match socks.get(&handle) {
        Some(sock) => {
            let inner = sock.inner.lock();
            !inner.recv_queue.is_empty() || inner.closed
        }
        None => false,
    }
}

pub fn cleanup_unix_socket(handle: u64) {
    let mut socks = UNIX_SOCKETS.lock();
    if let Some(sock) = socks.remove(&handle) {
        let mut inner = sock.inner.lock();
        inner.closed = true;
        if let Some(path) = &inner.bind_path {
            UNIX_BOUND.lock().remove(path.as_str());
        }
        let peer_handle = inner.peer;
        drop(inner);
        if let Some(ph) = peer_handle {
            if let Some(peer) = socks.get(&ph) {
                peer.inner.lock().closed = true;
                crate::task::scheduler::wake_pipe(ph);
            }
        }
        crate::task::scheduler::wake_pipe(handle);
    }
}

pub fn sys_socketpair(domain: u64, type_: u64, _protocol: u64, sv: *mut i32) -> Result<u64, Errno> {
    if domain != AF_UNIX as u64 {
        return Err(Errno::EAFNOSUPPORT);
    }
    let sock_type = match type_ {
        SOCK_STREAM => UnixSocketType::Stream,
        SOCK_DGRAM => UnixSocketType::Dgram,
        _ => return Err(Errno::EINVAL),
    };
    let h1 = alloc_handle();
    let h2 = alloc_handle();
    let sock1 = UnixSocket {
        inner: Arc::new(Mutex::new(UnixSocketInner {
            bind_path: None,
            peer: Some(h2),
            is_listening: false,
            backlog: Vec::new(),
            recv_queue: VecDeque::new(),
            closed: false,
            sock_type,
        })),
    };
    let sock2 = UnixSocket {
        inner: Arc::new(Mutex::new(UnixSocketInner {
            bind_path: None,
            peer: Some(h1),
            is_listening: false,
            backlog: Vec::new(),
            recv_queue: VecDeque::new(),
            closed: false,
            sock_type,
        })),
    };
    {
        let mut socks = UNIX_SOCKETS.lock();
        socks.insert(h1, sock1);
        socks.insert(h2, sock2);
    }
    let process = crate::syscalls::get_current_process().ok_or(Errno::ESRCH)?;
    let mut fd_table = process.fd_table.lock();
    let find_free = |table: &mut Vec<Option<crate::task::process::FileDescriptor>>| -> Option<u32> {
        for (i, slot) in table.iter_mut().enumerate() {
            if slot.is_none() {
                return Some(i as u32);
            }
        }
        let fd = table.len();
        table.push(None);
        Some(fd as u32)
    };
    let fd1 = find_free(&mut fd_table).ok_or(Errno::ENFILE)?;
    fd_table[fd1 as usize] = Some(crate::task::process::FileDescriptor::UnixSocket(h1, crate::task::process::SocketType::Unix));
    let fd2 = find_free(&mut fd_table).ok_or(Errno::ENFILE)?;
    fd_table[fd2 as usize] = Some(crate::task::process::FileDescriptor::UnixSocket(h2, crate::task::process::SocketType::Unix));
    drop(fd_table);
    let arr: [i32; 2] = [fd1 as i32, fd2 as i32];
    unsafe {
        user_access::copy_to_user(sv as *mut u8, core::slice::from_raw_parts(&arr as *const i32 as *const u8, 8))
            .map_err(|_| Errno::EFAULT)?;
    }
    Ok(0)
}
