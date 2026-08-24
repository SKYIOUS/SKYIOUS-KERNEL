//! Network internal helpers: constants, types, statics, and utility functions.
//! Extracted from net.rs to keep each module under 1k lines.

use super::errno;
use crate::sync::IrqSafeMutex as Mutex;
use alloc::vec::Vec;
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

// ─── Linux tcp_info struct ─────────────────────────────────────────

/// Linux-compatible tcp_info structure (104 bytes on 64-bit).
/// Returned by getsockopt(IPPROTO_TCP, TCP_INFO).
/// Layout matches the kernel's struct tcp_info for ABI compatibility
/// with tools like ss, netstat, curl.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TcpInfo {
    // Byte 0: connection state and control
    pub tcpi_state: u8,             // TCP state (ESTABLISHED, CLOSE_WAIT, etc.)
    pub tcpi_ca_state: u8,          // Congestion avoidance state
    pub tcpi_retransmits: u8,       // Unacknowledged retransmit counter
    pub tcpi_probes: u8,            // Probe timeout counter
    pub tcpi_backoff: u8,           // Backoff factor (0-7)
    pub tcpi_options: u8,           // Bitfield: sack, timestamps, wscale
    pub tcpi_snd_wscale: u8,        // Send window scale
    pub tcpi_rcv_wscale: u8,        // Receive window scale

    // Byte 8: timing
    pub tcpi_rto: u32,              // Retransmit timeout (us)
    pub tcpi_ato: u32,              // Predicted tick of most recent ACK (us)

    // Byte 16: MSS
    pub tcpi_snd_mss: u16,          // Send MSS
    pub tcpi_rcv_mss: u16,          // Receive MSS

    // Byte 20: unreceived/partially acked
    pub tcpi_unacked: u32,          // Segments not yet acknowledged
    pub tcpi_sacked: u32,           // Segments selectively ACKed
    pub tcpi_lost: u32,             // Segments considered lost
    pub tcpi_retrans: u32,          // Segments currently being retransmitted
    pub tcpi_fackets: u32,          // FACKed segments count

    // Byte 40: timestamps (ms since connection start)
    pub tcpi_last_data_sent: u32,   // Time since last data sent
    pub tcpi_last_ack_sent: u32,    // Time since last ACK sent
    pub tcpi_last_data_recv: u32,   // Time since last data received
    pub tcpi_last_ack_recv: u32,    // Time since last ACK received

    // Byte 56: RTT
    pub tcpi_rtt: u32,              // Smoothed RTT (us)
    pub tcpi_rttvar: u32,           // RTT variance (us)

    // Byte 64: ssthresh
    pub tcpi_snd_ssthresh: u32,     // Slow start threshold
    pub tcpi_rcv_ssthresh: u32,     // Receive window threshold

    // Byte 72: misc
    pub tcpi_reordering: u32,       // Reordering threshold
    pub tcpi_rcv_rtt: u32,          // Receive-side RTT estimate
    pub tcpi_rcv_space: u32,        // Receive window space

    // Byte 84: total counters
    pub tcpi_total_retrans: u32,    // Total retransmissions since connect

    // Byte 88: pacing rate
    pub tcpi_pacing_rate: u64,      // Pacing rate (bytes/s), 0 = not measured

    // Byte 96: application-level byte counters
    pub tcpi_bytes_acked: u64,      // Total bytes ACKed (application data)
    pub tcpi_bytes_received: u64,   // Total bytes received (application data)

    // Byte 112: segment counters
    pub tcpi_segs_out: u32,         // Segments sent
    pub tcpi_segs_in: u32,          // Segments received
}

/// TCP connection state constants (Linux ABI-compatible)
pub(crate) const TCP_ESTABLISHED: u8 = 1;
pub(crate) const TCP_SYN_SENT: u8 = 2;
pub(crate) const TCP_SYN_RECV: u8 = 3;
pub(crate) const TCP_FIN_WAIT1: u8 = 4;
pub(crate) const TCP_FIN_WAIT2: u8 = 5;
pub(crate) const TCP_TIME_WAIT: u8 = 6;
pub(crate) const TCP_CLOSE: u8 = 7;
pub(crate) const TCP_CLOSE_WAIT: u8 = 8;
pub(crate) const TCP_LAST_ACK: u8 = 9;
pub(crate) const TCP_LISTEN: u8 = 10;
pub(crate) const TCP_CLOSING: u8 = 11;

/// TCP options bitmask for tcp_info.tcpi_options
pub(crate) const TCPOPT_TIMESTAMP: u8 = 1 << 0;
pub(crate) const TCPOPT_SACK: u8 = 1 << 1;
pub(crate) const TCPOPT_WSCALE: u8 = 1 << 2;

/// Congestion avoidance states (tcp_info.tcpi_ca_state)
pub(crate) const TCP_CA_OPEN: u8 = 0;
pub(crate) const TCP_CA_DISORDER: u8 = 1;
pub(crate) const TCP_CA_CWR: u8 = 2;
pub(crate) const TCP_CA_RECOVERY: u8 = 3;
pub(crate) const TCP_CA_LOSS: u8 = 4;

// ─── Per-connection TCP statistics ─────────────────────────────────

/// Tracks per-connection TCP statistics that smoltcp does not expose.
/// Updated by sendto_internal / recvfrom_internal.
#[derive(Clone)]
pub(crate) struct TcpConnectionStats {
    pub bytes_acked: u64,
    pub bytes_received: u64,
    pub segs_out: u32,
    pub segs_in: u32,
    pub total_retrans: u32,
    pub connect_tick: u64,          // boot tick when connection was established
    pub last_data_sent_tick: u64,
    pub last_data_recv_tick: u64,
    pub last_ack_sent_tick: u64,
    pub last_ack_recv_tick: u64,
    pub reordering: u32,
    pub snd_ssthresh: u32,
    pub rcv_ssthresh: u32,
}

impl Default for TcpConnectionStats {
    fn default() -> Self {
        Self {
            bytes_acked: 0,
            bytes_received: 0,
            segs_out: 0,
            segs_in: 0,
            total_retrans: 0,
            connect_tick: 0,
            last_data_sent_tick: 0,
            last_data_recv_tick: 0,
            last_ack_sent_tick: 0,
            last_ack_recv_tick: 0,
            reordering: 3,
            snd_ssthresh: 65535,
            rcv_ssthresh: 65535,
        }
    }
}

/// Global TCP connection stats: (pid, handle) → TcpConnectionStats
lazy_static::lazy_static! {
    pub(crate) static ref TCP_STATS: Mutex<HashMap<(u64, SocketHandle), TcpConnectionStats>> =
        Mutex::new(HashMap::new());
}

/// Record the current tick as the connect time for a new TCP connection.
pub(crate) fn tcp_stats_on_connect(pid: u64, handle: SocketHandle) {
    let tick = crate::hal::timer::current_time_us();
    let mut stats = TCP_STATS.lock();
    let entry = stats.entry((pid, handle)).or_insert_with(TcpConnectionStats::default);
    entry.connect_tick = tick;
    // Initialize congestion control for this connection.
    crate::net::tcp_congestion::create(handle.id(), 1460);
}

/// Record bytes sent and segment count for a TCP connection.
pub(crate) fn tcp_stats_record_send(pid: u64, handle: SocketHandle, bytes: u64) {
    let tick = crate::hal::timer::current_time_us();
    let mut stats = TCP_STATS.lock();
    let entry = stats.entry((pid, handle)).or_insert_with(TcpConnectionStats::default);
    entry.bytes_acked += bytes;
    entry.segs_out += 1;
    entry.last_data_sent_tick = tick;
}

/// Record bytes received and segment count for a TCP connection.
pub(crate) fn tcp_stats_record_recv(pid: u64, handle: SocketHandle, bytes: u64) {
    let tick = crate::hal::timer::current_time_us();
    let mut stats = TCP_STATS.lock();
    let entry = stats.entry((pid, handle)).or_insert_with(TcpConnectionStats::default);
    entry.bytes_received += bytes;
    entry.segs_in += 1;
    entry.last_data_recv_tick = tick;
}

/// Remove TCP stats entry when socket is closed.
pub(crate) fn tcp_stats_remove(pid: u64, handle: SocketHandle) {
    TCP_STATS.lock().remove(&(pid, handle));
    crate::net::tcp_congestion::remove(handle.id());
}

/// Build a Linux-compatible tcp_info from the smoltcp socket state + tracked stats.
pub(crate) fn build_tcp_info(pid: u64, handle: SocketHandle) -> TcpInfo {
    // Step 1: Read tracked stats under TCP_STATS lock (release before locking SOCKETS)
    let stats_snapshot: TcpConnectionStats = {
        let mut out = TcpConnectionStats::default();
        {
            let stats = TCP_STATS.lock();
            if let Some(s) = stats.get(&(pid, handle)) {
                out = s.clone();
            }
        }
        out
    };

    // Step 2: Query smoltcp TCP socket state
    let mut sockets = crate::net::SOCKETS.lock();
    let mut info = TcpInfo::default();

    // Query smoltcp TCP socket state — match enum directly, no heap allocation
    let socket_state = with_tcp_mut(&mut sockets, handle, |socket| {
        let state = socket.state();
        let is_open = socket.is_open();
        (state, is_open)
    });

    if let Some((state, is_open)) = socket_state {
        use smoltcp::socket::tcp::State;
        info.tcpi_state = match state {
            State::Closed => if is_open { TCP_LISTEN } else { TCP_CLOSE },
            State::Listen => TCP_LISTEN,
            State::SynSent => TCP_SYN_SENT,
            State::SynReceived => TCP_SYN_RECV,
            State::Established => TCP_ESTABLISHED,
            State::FinWait1 => TCP_FIN_WAIT1,
            State::FinWait2 => TCP_FIN_WAIT2,
            State::Closing => TCP_CLOSING,
            State::CloseWait => TCP_CLOSE_WAIT,
            State::LastAck => TCP_LAST_ACK,
            State::TimeWait => TCP_TIME_WAIT,
        };
    } else {
        // Socket not found — return closed
        info.tcpi_state = TCP_CLOSE;
    }

    // Congestion state based on retransmit history
    info.tcpi_ca_state = if stats_snapshot.total_retrans > 10 {
        TCP_CA_RECOVERY
    } else if stats_snapshot.total_retrans > 0 {
        TCP_CA_DISORDER
    } else {
        TCP_CA_OPEN
    };

    // TCP options and window scaling
    info.tcpi_options = TCPOPT_TIMESTAMP | TCPOPT_SACK | TCPOPT_WSCALE;
    info.tcpi_snd_wscale = 7;  // 128x window scaling
    info.tcpi_rcv_wscale = 7;

    // MSS: Ethernet MTU 1500 minus IP+TCP headers
    info.tcpi_snd_mss = 1460;
    info.tcpi_rcv_mss = 1460;

    // RTT: smoothed estimate in microseconds
    info.tcpi_rtt = 1_000;      // 1ms base RTT (typical LAN)
    info.tcpi_rttvar = 500;     // 0.5ms variance
    info.tcpi_rto = 200_000;    // 200ms minimum RTO (Linux standard)
    info.tcpi_ato = 400_000;    // 400ms ACK timeout (typical delayed-ACK timer)

    // ssthresh from tracked stats
    info.tcpi_snd_ssthresh = stats_snapshot.snd_ssthresh;
    info.tcpi_rcv_ssthresh = stats_snapshot.rcv_ssthresh;
    info.tcpi_reordering = stats_snapshot.reordering;

    // Byte and segment counters from tracked stats
    info.tcpi_bytes_acked = stats_snapshot.bytes_acked;
    info.tcpi_bytes_received = stats_snapshot.bytes_received;
    info.tcpi_segs_out = stats_snapshot.segs_out;
    info.tcpi_segs_in = stats_snapshot.segs_in;
    info.tcpi_total_retrans = stats_snapshot.total_retrans;

    // Timestamps: microseconds since boot → convert to milliseconds for userspace
    let now = crate::hal::timer::current_time_us();
    let connect_tick = stats_snapshot.connect_tick;
    if connect_tick > 0 {
        info.tcpi_last_data_sent = if stats_snapshot.last_data_sent_tick > 0 {
            (now.saturating_sub(stats_snapshot.last_data_sent_tick) / 1000) as u32
        } else {
            (now.saturating_sub(connect_tick) / 1000) as u32
        };
        info.tcpi_last_data_recv = if stats_snapshot.last_data_recv_tick > 0 {
            (now.saturating_sub(stats_snapshot.last_data_recv_tick) / 1000) as u32
        } else {
            (now.saturating_sub(connect_tick) / 1000) as u32
        };
        info.tcpi_last_ack_sent = if stats_snapshot.last_ack_sent_tick > 0 {
            (now.saturating_sub(stats_snapshot.last_ack_sent_tick) / 1000) as u32
        } else {
            (now.saturating_sub(connect_tick) / 1000) as u32
        };
        info.tcpi_last_ack_recv = if stats_snapshot.last_ack_recv_tick > 0 {
            (now.saturating_sub(stats_snapshot.last_ack_recv_tick) / 1000) as u32
        } else {
            (now.saturating_sub(connect_tick) / 1000) as u32
        };
    }

    // Receive window space estimate
    info.tcpi_rcv_space = 65535;

    info
}

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

// ─── SO_REUSEPORT infrastructure ───────────────────────────────────

/// Per-socket flags for TCP/UDP sockets.
pub(crate) struct SocketFlags {
    pub reuse_port: bool,
}

impl Default for SocketFlags {
    fn default() -> Self {
        Self { reuse_port: false }
    }
}

/// Global per-socket flags: (pid, handle) → SocketFlags
lazy_static::lazy_static! {
    pub(crate) static ref SOCKET_FLAGS: Mutex<HashMap<(u64, SocketHandle), SocketFlags>> =
        Mutex::new(HashMap::new());
}

/// A group of sockets bound to the same port with SO_REUSEPORT.
/// Provides kernel-level load balancing across the group.
pub(crate) struct ReusePortGroup {
    /// TCP sockets in this group: (pid, handle, endpoint)
    pub tcp_sockets: Vec<(u64, SocketHandle, IpEndpoint)>,
    /// UDP sockets in this group: (pid, handle, endpoint)
    pub udp_sockets: Vec<(u64, SocketHandle, IpEndpoint)>,
    /// Round-robin counter for UDP load balancing
    pub udp_rr_counter: u64,
}

impl ReusePortGroup {
    pub fn new() -> Self {
        Self {
            tcp_sockets: Vec::new(),
            udp_sockets: Vec::new(),
            udp_rr_counter: 0,
        }
    }
}

/// Global SO_REUSEPORT group registry: port → ReusePortGroup
lazy_static::lazy_static! {
    pub(crate) static ref REUSEPORT_SOCKETS: Mutex<HashMap<u16, ReusePortGroup>> =
        Mutex::new(HashMap::new());
}

/// Simple hash function for consistent hashing across sockets.
/// Used for TCP accept load balancing in SO_REUSEPORT groups.
pub(crate) fn simple_hash(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325; // FNV-1a basis
    for &byte in data {
        h ^= byte as u64;
        h = h.wrapping_mul(0x100000001b3); // FNV-1a prime
    }
    h
}

/// Check if a socket has SO_REUSEPORT set.
pub(crate) fn has_reuse_port(pid: u64, handle: SocketHandle) -> bool {
    let flags = SOCKET_FLAGS.lock();
    flags.get(&(pid, handle)).map(|f| f.reuse_port).unwrap_or(false)
}

/// Set SO_REUSEPORT flag on a socket.
pub(crate) fn set_reuse_port(pid: u64, handle: SocketHandle, enable: bool) {
    let mut flags = SOCKET_FLAGS.lock();
    let entry = flags.entry((pid, handle)).or_insert_with(SocketFlags::default);
    entry.reuse_port = enable;
}

/// Remove socket from the REUSEPORT group registry when closed.
pub(crate) fn remove_from_reuseport(pid: u64, handle: SocketHandle) {
    let mut groups = REUSEPORT_SOCKETS.lock();
    for (_port, group) in groups.iter_mut() {
        group.tcp_sockets.retain(|&(h_pid, h, _)| h_pid != pid || h != handle);
        group.udp_sockets.retain(|&(h_pid, h, _)| h_pid != pid || h != handle);
    }
    groups.retain(|_, group| !group.tcp_sockets.is_empty() || !group.udp_sockets.is_empty());
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
            // Congestion control: limit send to cwnd budget.
            let budget = crate::net::tcp_congestion::send_budget(handle.id());
            if budget == 0 {
                return errno::Errno::EAGAIN as u64;
            }
            let send_len = core::cmp::min(data.len(), budget as usize);
            let send_data = &data[..send_len];
            if with_tcp_mut(sockets, handle, |socket| {
                if socket.may_send() {
                    let result = socket.send(|slice| {
                        let n = core::cmp::min(slice.len(), send_data.len());
                        slice[..n].copy_from_slice(&send_data[..n]);
                        (n, true)
                    });
                    if result.unwrap_or(false) {
                        crate::net::tcp_congestion::on_send(handle.id(), send_len as u32);
                        return send_len as u64;
                    }
                }
                errno::Errno::EAGAIN as u64
            }).is_some() {
                return send_len as u64;
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
        let fd_table = process.files.lock().fd_table.clone();
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
