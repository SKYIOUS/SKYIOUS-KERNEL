//! Socket management and state machine.
//!
//! Maps file descriptors to smoltcp sockets. Manages connection state,
//! send/receive buffers, and blocking operations via wait queues.

/// Socket type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SockType {
    Stream,   // TCP (SOCK_STREAM)
    Datagram, // UDP (SOCK_DGRAM)
    Raw,      // Raw socket (SOCK_RAW)
}

/// Socket state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SockState {
    Unused,
    Created,
    Bound,
    Listening,
    Connecting,
    Connected,
    Closed,
}
