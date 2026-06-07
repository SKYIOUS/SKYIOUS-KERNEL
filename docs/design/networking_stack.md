# Network Stack Architecture

The SkyOS network stack is a modular, multi-layer implementation designed for both performance and simplicity.

## Layer Structure

The network stack follows the traditional layered model:

1. **Physical Layer**: Drivers (e1000, VirtIO-net) handle DMA and interrupt processing
2. **Link Layer**: Ethernet framing and ARP for address resolution
3. **Network Layer**: IPv4 routing and packet forwarding
4. **Transport Layer**: TCP connection management and UDP datagram delivery
5. **Socket Layer**: BSD socket API mapping to transport protocols

## Socket API

The socket API maps to the VFS layer. Sockets are represented as special file descriptors:

```rust
pub enum SocketDomain { IPv4, IPv6, Unix }
pub enum SocketType { Stream, Dgram, Raw }
pub enum SocketProtocol { Tcp, Udp, Icmp }
```

## TCP Implementation

The TCP implementation includes:
- Sliding window flow control
- Nagle's algorithm for coalescing small packets
- Fast retransmit and recovery
- Selective ACK (SACK) support
- Congestion control (CUBIC algorithm)

## Packet Buffering

Network buffers use a `mbuf`-style structure with shared ownership:

```rust
pub struct NetBuf {
    data: Arc<Vec<u8>>,
    offset: usize,
    length: usize,
    next: Option<Box<NetBuf>>,
}
```

This structure supports zero-copy packet forwarding by adjusting offsets rather than copying data.

## Future Plans

- IPv6 support
- TCP offload engine (TOE) for capable NICs
- Network stack virtualization for container networking
- WireGuard VPN integration
