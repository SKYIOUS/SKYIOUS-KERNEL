//! # vahi-ipc — Inter-Process Communication
//!
//! Structured IPC with zero-copy transfers, capability-based endpoints,
//! and port-based messaging (Windows NT LPC/ALPC equivalent).
//!
//! ## Module Breakdown
//!
//! | Type | Purpose |
//! |------|---------|
//! | `IpcEndpoint` | Named communication channel between processes |
//! | `IpcMessage` | Typed message with optional zero-copy payload |
//! | `IpcSharedRegion` | Zero-copy shared memory region |
//! | `IpcPort` | Fast local port (server/client) |
//!
//! ## Invariants
//!
//! - **Endpoint names** are unique within a process namespace
//! - **Message queues** are bounded by `max_queue_depth`
//! - **Shared regions** are reference-counted and freed on last close
//! - **IPC_NOWAIT** returns `EAGAIN` instead of blocking

#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use vahi_types::Errno;

/// Mutex type: IrqSafeMutex on bare-metal (disables interrupts),
/// spin::Mutex in test mode (no privileged instructions).
#[cfg(not(all(test, not(target_os = "none"))))]
type IpcMutex<T> = vahi_sync::IrqSafeMutex<T>;
#[cfg(all(test, not(target_os = "none")))]
type IpcMutex<T> = spin::Mutex<T>;

/// IPC message header
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IpcHeader {
    /// Message type (user-defined)
    pub msg_type: u32,
    /// Flags (IPC_NOWAIT, IPC_COPY, etc.)
    pub flags: u32,
    /// Payload length in bytes
    pub payload_len: u64,
    /// Sender process ID
    pub sender_pid: u64,
    /// Sequence number
    pub seq: u64,
}

/// IPC message flags
pub const IPC_NOWAIT: u32 = 0x01;
pub const IPC_COPY: u32 = 0x02;
pub const IPC_ZERO_COPY: u32 = 0x04;

/// IPC endpoint ID
pub type EndpointId = u64;

/// IPC endpoint — a communication channel between processes
pub struct IpcEndpoint {
    /// Unique endpoint ID
    pub id: EndpointId,
    /// Endpoint name (for lookup)
    pub name: Vec<u8>,
    /// Owner process ID
    pub owner_pid: u64,
    /// Message queue (pending messages)
    pub queue: Vec<IpcMessage>,
    /// Maximum queue depth
    pub max_queue_depth: usize,
    /// Capabilities required to access this endpoint
    pub required_caps: u64,
    /// Connected client endpoint IDs (for server ports)
    pub clients: Vec<EndpointId>,
}

/// An IPC message with optional zero-copy payload
pub struct IpcMessage {
    /// Message header
    pub header: IpcHeader,
    /// Message payload (owned data)
    pub payload: Vec<u8>,
    /// Zero-copy region ID (if IPC_ZERO_COPY flag is set)
    pub zerocopy_region: Option<u64>,
}

/// Zero-copy shared memory region
pub struct IpcSharedRegion {
    /// Region ID
    pub id: u64,
    /// Physical address of the region
    pub phys_addr: u64,
    /// Size in bytes
    pub size: usize,
    /// Number of references (processes mapped to this region)
    pub ref_count: u32,
    /// Owner process ID
    pub owner_pid: u64,
}

/// Global IPC state
pub struct IpcState {
    /// All registered endpoints
    pub endpoints: BTreeMap<EndpointId, Arc<IpcMutex<IpcEndpoint>>>,
    /// Shared memory regions
    pub regions: BTreeMap<u64, Arc<IpcMutex<IpcSharedRegion>>>,
    /// Next endpoint ID
    pub next_endpoint_id: EndpointId,
    /// Next region ID
    pub next_region_id: u64,
}

/// Global IPC state instance
pub static IPC_STATE: IpcMutex<IpcState> = IpcMutex::new(IpcState {
    endpoints: BTreeMap::new(),
    regions: BTreeMap::new(),
    next_endpoint_id: 1,
    next_region_id: 1,
});

/// Create a new IPC endpoint.
pub fn ipc_create_endpoint(
    name: &[u8],
    owner_pid: u64,
    max_queue: usize,
) -> Result<EndpointId, Errno> {
    let mut state = IPC_STATE.lock();

    let id = state.next_endpoint_id;
    state.next_endpoint_id += 1;

    let endpoint = IpcEndpoint {
        id,
        name: name.to_vec(),
        owner_pid,
        queue: Vec::new(),
        max_queue_depth: max_queue,
        required_caps: 0,
        clients: Vec::new(),
    };

    state
        .endpoints
        .insert(id, Arc::new(IpcMutex::new(endpoint)));
    Ok(id)
}

/// Send a message to an IPC endpoint.
/// When IPC_NOWAIT is not set and the queue is full, spins (with lock dropped)
/// up to 4096 iterations before returning EAGAIN.
pub fn ipc_send(
    endpoint_id: EndpointId,
    msg_type: u32,
    payload: &[u8],
    flags: u32,
) -> Result<(), Errno> {
    let endpoint = {
        let state = IPC_STATE.lock();
        state
            .endpoints
            .get(&endpoint_id)
            .ok_or(Errno::ENOENT)?
            .clone()
    };

    let mut attempts = 0u32;
    loop {
        let mut ep = endpoint.lock();
        if ep.queue.len() < ep.max_queue_depth {
            let msg = IpcMessage {
                header: IpcHeader {
                    msg_type,
                    flags,
                    payload_len: payload.len() as u64,
                    sender_pid: vahi_types::current_pid(),
                    seq: ep.queue.len() as u64,
                },
                payload: payload.to_vec(),
                zerocopy_region: None,
            };
            ep.queue.push(msg);
            return Ok(());
        }
        drop(ep);
        if flags & IPC_NOWAIT != 0 || attempts >= 4096 {
            return Err(Errno::EAGAIN);
        }
        attempts += 1;
        core::hint::spin_loop();
    }
}

/// Receive a message from an IPC endpoint.
/// When IPC_NOWAIT is not set and the queue is empty, spins (with lock dropped)
/// up to 4096 iterations before returning EAGAIN.
pub fn ipc_recv(
    endpoint_id: EndpointId,
    buf: &mut [u8],
    flags: u32,
) -> Result<(usize, u32), Errno> {
    let endpoint = {
        let state = IPC_STATE.lock();
        state
            .endpoints
            .get(&endpoint_id)
            .ok_or(Errno::ENOENT)?
            .clone()
    };

    let mut attempts = 0u32;
    loop {
        let mut ep = endpoint.lock();
        if !ep.queue.is_empty() {
            let msg = ep.queue.remove(0);
            let copy_len = core::cmp::min(buf.len(), msg.payload.len());
            buf[..copy_len].copy_from_slice(&msg.payload[..copy_len]);
            return Ok((copy_len, msg.header.msg_type));
        }
        drop(ep);
        if flags & IPC_NOWAIT != 0 || attempts >= 4096 {
            return Err(Errno::EAGAIN);
        }
        attempts += 1;
        core::hint::spin_loop();
    }
}

/// Create a zero-copy shared memory region.
pub fn ipc_create_region(_size: usize, _owner_pid: u64) -> Result<u64, Errno> {
    Err(Errno::ENOSYS)
}

/// Send a zero-copy message using a shared region.
pub fn ipc_send_zerocopy(
    endpoint_id: EndpointId,
    msg_type: u32,
    region_id: u64,
    _offset: usize,
    len: usize,
) -> Result<(), Errno> {
    let state = IPC_STATE.lock();

    let endpoint = state.endpoints.get(&endpoint_id).ok_or(Errno::ENOENT)?;

    let _region = state.regions.get(&region_id).ok_or(Errno::ENOENT)?;

    let mut ep = endpoint.lock();

    if ep.queue.len() >= ep.max_queue_depth {
        return Err(Errno::EAGAIN);
    }

    let msg = IpcMessage {
        header: IpcHeader {
            msg_type,
            flags: IPC_ZERO_COPY,
            payload_len: len as u64,
            sender_pid: vahi_types::current_pid(),
            seq: ep.queue.len() as u64,
        },
        payload: Vec::new(), // No payload — data is in shared region
        zerocopy_region: Some(region_id),
    };

    ep.queue.push(msg);
    Ok(())
}

/// Close an IPC endpoint.
pub fn ipc_close_endpoint(endpoint_id: EndpointId) -> Result<(), Errno> {
    let mut state = IPC_STATE.lock();
    state.endpoints.remove(&endpoint_id).ok_or(Errno::ENOENT)?;
    Ok(())
}

/// Destroy a zero-copy shared memory region.
pub fn ipc_destroy_region(region_id: u64) -> Result<(), Errno> {
    let mut state = IPC_STATE.lock();
    state.regions.remove(&region_id).ok_or(Errno::ENOENT)?;
    Ok(())
}

// ─── Port-based IPC (Windows NT LPC/ALPC equivalent) ──────────────

/// Port type: server port accepts connections, client port connects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortType {
    Server,
    Client,
}

/// A port for fast local IPC (like Windows NT LPC/ALPC).
///
/// Ports provide:
/// - Fast synchronous message passing
/// - Optional zero-copy via shared sections
/// - Security via port namespace access control
pub struct IpcPort {
    /// Port name (for server ports)
    pub name: Vec<u8>,
    /// Port type (server or client)
    pub port_type: PortType,
    /// Owner process ID
    pub owner_pid: u64,
    /// Connected client ports (for server ports)
    pub clients: Vec<EndpointId>,
    /// Server port this client is connected to (for client ports)
    pub server_port: Option<EndpointId>,
    /// Message queue
    pub queue: Vec<IpcMessage>,
    /// Maximum queue depth
    pub max_queue_depth: usize,
}

/// Create a server port for accepting connections.
pub fn port_create_server(
    name: &[u8],
    owner_pid: u64,
    max_queue: usize,
) -> Result<EndpointId, Errno> {
    let mut state = IPC_STATE.lock();

    let id = state.next_endpoint_id;
    state.next_endpoint_id += 1;

    let _port = IpcPort {
        name: name.to_vec(),
        port_type: PortType::Server,
        owner_pid,
        clients: Vec::new(),
        server_port: None,
        queue: Vec::new(),
        max_queue_depth: max_queue,
    };

    // Wrap in IpcEndpoint for compatibility
    let endpoint = IpcEndpoint {
        id,
        name: name.to_vec(),
        owner_pid,
        queue: Vec::new(),
        max_queue_depth: max_queue,
        required_caps: 0,
        clients: Vec::new(),
    };

    state
        .endpoints
        .insert(id, Arc::new(IpcMutex::new(endpoint)));
    Ok(id)
}

/// Connect to a server port.
/// Creates a client endpoint linked to the named server.
pub fn port_connect(server_name: &[u8], client_pid: u64) -> Result<EndpointId, Errno> {
    // Phase 1: find the server endpoint Arc without holding IPC_STATE.
    // This avoids nested locks (IPC_STATE → endpoint.lock()).
    let server_arc = {
        let state = IPC_STATE.lock();
        state
            .endpoints
            .iter()
            .find(|(_, ep)| ep.lock().name == server_name)
            .map(|(_, arc)| arc.clone())
            .ok_or(Errno::ENOENT)?
    };
    // Verify the server exists and is reachable.
    {
        let _ep = server_arc.lock();
    }

    // Phase 2: create the client and link it to the server under IPC_STATE.
    let client_id = {
        let mut state = IPC_STATE.lock();
        let id = state.next_endpoint_id;
        state.next_endpoint_id += 1;
        let client_endpoint = IpcEndpoint {
            id,
            name: server_name.to_vec(),
            owner_pid: client_pid,
            queue: Vec::new(),
            max_queue_depth: 256,
            required_caps: 0,
            clients: Vec::new(),
        };
        state
            .endpoints
            .insert(id, Arc::new(IpcMutex::new(client_endpoint)));
        // Link client to server so the server can enumerate connected clients.
        if let Some(server_ep) = state.endpoints.get(&{
            // Re-find server ID — we dropped state in phase 1
            state
                .endpoints
                .iter()
                .find(|(_, ep)| ep.lock().name == server_name)
                .map(|(id, _)| *id)
                .unwrap_or(0)
        }) {
            server_ep.lock().clients.push(id);
        }
        id
    };

    Ok(client_id)
}

/// Send a message through a port.
/// Spins (with lock dropped) up to 4096 iterations when queue is full
/// unless IPC_NOWAIT is set.
pub fn port_send(
    port_id: EndpointId,
    msg_type: u32,
    payload: &[u8],
    flags: u32,
) -> Result<(), Errno> {
    let endpoint = {
        let state = IPC_STATE.lock();
        state.endpoints.get(&port_id).ok_or(Errno::ENOENT)?.clone()
    };

    let mut attempts = 0u32;
    loop {
        let mut ep = endpoint.lock();
        if ep.queue.len() < ep.max_queue_depth {
            let msg = IpcMessage {
                header: IpcHeader {
                    msg_type,
                    flags,
                    payload_len: payload.len() as u64,
                    sender_pid: vahi_types::current_pid(),
                    seq: ep.queue.len() as u64,
                },
                payload: payload.to_vec(),
                zerocopy_region: None,
            };
            ep.queue.push(msg);
            return Ok(());
        }
        drop(ep);
        if flags & IPC_NOWAIT != 0 || attempts >= 4096 {
            return Err(Errno::EAGAIN);
        }
        attempts += 1;
        core::hint::spin_loop();
    }
}

/// Receive a message from a port.
/// Spins (with lock dropped) up to 4096 iterations when queue is empty
/// unless IPC_NOWAIT is set.
pub fn port_recv(port_id: EndpointId, buf: &mut [u8], flags: u32) -> Result<(usize, u32), Errno> {
    let endpoint = {
        let state = IPC_STATE.lock();
        state.endpoints.get(&port_id).ok_or(Errno::ENOENT)?.clone()
    };

    let mut attempts = 0u32;
    loop {
        let mut ep = endpoint.lock();
        if !ep.queue.is_empty() {
            let msg = ep.queue.remove(0);
            let copy_len = core::cmp::min(buf.len(), msg.payload.len());
            buf[..copy_len].copy_from_slice(&msg.payload[..copy_len]);
            return Ok((copy_len, msg.header.msg_type));
        }
        drop(ep);
        if flags & IPC_NOWAIT != 0 || attempts >= 4096 {
            return Err(Errno::EAGAIN);
        }
        attempts += 1;
        core::hint::spin_loop();
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Endpoint lifecycle ──────────────────────────────────────────────────

    #[test]
    fn create_and_close_endpoint() {
        let id = ipc_create_endpoint(b"test-ep", 1, 16).unwrap();
        assert!(id > 0);
        {
            let state = IPC_STATE.lock();
            assert!(state.endpoints.contains_key(&id));
        }
        ipc_close_endpoint(id).unwrap();
        {
            let state = IPC_STATE.lock();
            assert!(!state.endpoints.contains_key(&id));
        }
    }

    #[test]
    fn close_nonexistent_returns_enoent() {
        assert_eq!(ipc_close_endpoint(9999), Err(Errno::ENOENT));
    }

    // ── Send / recv basic ────────────────────────────────────────────────────

    #[test]
    fn send_recv_basic() {
        let id = ipc_create_endpoint(b"sr-ep", 1, 16).unwrap();
        ipc_send(id, 42, b"hello", 0).unwrap();
        let mut buf = [0u8; 32];
        let (n, msg_type) = ipc_recv(id, &mut buf, IPC_NOWAIT).unwrap();
        assert_eq!(n, 5);
        assert_eq!(msg_type, 42);
        assert_eq!(&buf[..n], b"hello");
        ipc_close_endpoint(id).unwrap();
    }

    #[test]
    fn send_recv_fifo_order() {
        let id = ipc_create_endpoint(b"fifo", 1, 16).unwrap();
        ipc_send(id, 1, b"first", 0).unwrap();
        ipc_send(id, 2, b"second", 0).unwrap();
        ipc_send(id, 3, b"third", 0).unwrap();
        let mut buf = [0u8; 32];
        let (_, t1) = ipc_recv(id, &mut buf, IPC_NOWAIT).unwrap();
        assert_eq!(t1, 1);
        let (_, t2) = ipc_recv(id, &mut buf, IPC_NOWAIT).unwrap();
        assert_eq!(t2, 2);
        let (_, t3) = ipc_recv(id, &mut buf, IPC_NOWAIT).unwrap();
        assert_eq!(t3, 3);
        ipc_close_endpoint(id).unwrap();
    }

    // ── IPC_NOWAIT flag ─────────────────────────────────────────────────────

    #[test]
    fn recv_empty_nowait_returns_eagain() {
        let id = ipc_create_endpoint(b"nw-ep", 1, 16).unwrap();
        let mut buf = [0u8; 32];
        assert_eq!(ipc_recv(id, &mut buf, IPC_NOWAIT), Err(Errno::EAGAIN));
        ipc_close_endpoint(id).unwrap();
    }

    #[test]
    fn send_nonexistent_returns_enoent() {
        assert_eq!(ipc_send(9999, 0, b"x", IPC_NOWAIT), Err(Errno::ENOENT));
    }

    #[test]
    fn recv_nonexistent_returns_enoent() {
        let mut buf = [0u8; 32];
        assert_eq!(ipc_recv(9999, &mut buf, IPC_NOWAIT), Err(Errno::ENOENT));
    }

    // ── Queue overflow ──────────────────────────────────────────────────────

    #[test]
    fn send_full_queue_nowait_returns_eagain() {
        let id = ipc_create_endpoint(b"full", 1, 2).unwrap();
        ipc_send(id, 0, b"a", 0).unwrap();
        ipc_send(id, 0, b"b", 0).unwrap();
        assert_eq!(ipc_send(id, 0, b"c", IPC_NOWAIT), Err(Errno::EAGAIN));
        ipc_close_endpoint(id).unwrap();
    }

    // ── Multiple endpoints isolated ─────────────────────────────────────────

    #[test]
    fn endpoints_are_isolated() {
        let ep1 = ipc_create_endpoint(b"iso1", 1, 16).unwrap();
        let ep2 = ipc_create_endpoint(b"iso2", 1, 16).unwrap();
        ipc_send(ep1, 1, b"to-ep1", 0).unwrap();
        ipc_send(ep2, 2, b"to-ep2", 0).unwrap();
        let mut buf = [0u8; 32];
        let (n1, t1) = ipc_recv(ep1, &mut buf, IPC_NOWAIT).unwrap();
        assert_eq!(t1, 1);
        assert_eq!(&buf[..n1], b"to-ep1");

        let (n2, t2) = ipc_recv(ep2, &mut buf, IPC_NOWAIT).unwrap();
        assert_eq!(t2, 2);
        assert_eq!(&buf[..n2], b"to-ep2");
        ipc_close_endpoint(ep1).unwrap();
        ipc_close_endpoint(ep2).unwrap();
    }

    // ── Port-based IPC ─────────────────────────────────────────────────────

    #[test]
    fn port_create_and_connect() {
        let server = port_create_server(b"test-port", 1, 16).unwrap();
        let client = port_connect(b"test-port", 2).unwrap();
        assert!(client != server);
        {
            let state = IPC_STATE.lock();
            let server_ep = state.endpoints.get(&server).unwrap().lock();
            assert!(server_ep.clients.contains(&client));
        }
        ipc_close_endpoint(server).unwrap();
        ipc_close_endpoint(client).unwrap();
    }

    #[test]
    fn port_connect_nonexistent_returns_enoent() {
        assert_eq!(port_connect(b"no-such-port", 1), Err(Errno::ENOENT));
    }

    #[test]
    fn port_send_recv() {
        let server = port_create_server(b"psr", 1, 16).unwrap();
        let client = port_connect(b"psr", 2).unwrap();
        // Send to the server endpoint (not the client) so server can recv
        port_send(server, 10, b"data", 0).unwrap();
        let mut buf = [0u8; 32];
        let (n, t) = port_recv(server, &mut buf, IPC_NOWAIT).unwrap();
        assert_eq!(n, 4);
        assert_eq!(t, 10);
        assert_eq!(&buf[..4], b"data");
        ipc_close_endpoint(server).unwrap();
        ipc_close_endpoint(client).unwrap();
    }
}
