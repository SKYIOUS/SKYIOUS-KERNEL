# vahi-net

Networking stack — TCP/UDP via smoltcp, DNS, DHCP, Unix sockets, congestion control.

## Modules

| Module | Lines | Contents |
|--------|-------|----------|
| `mod.rs` | 208 | Network interface, socket table, packet dispatch |
| `tcp_congestion.rs` | 221 | TCP Reno (cwnd, slow start, fast recovery) |
| `dns.rs` | 135 | DNS resolver with caching |
| `unix.rs` | 491 | Unix domain sockets (AF_UNIX) |
| `zerocopy.rs` | 563 | Zero-copy packet buffers |
| `rss.rs` | 292 | Receive-Side Scaling for multi-queue NICs |
| `dhcp.rs` | 17 | DHCP client |

## Key Types

| Type | Purpose |
|------|---------|
| `TcpReno` | TCP Reno congestion control state machine |
| `DnsEntry` | DNS cache entry with TTL |
| `SockType` | Stream/Datagram/Raw |
| `TcpState` | RFC 793 TCP connection states |
| `SocketAddr` | IPv4/Unix socket addresses |

## Concurrency Model

```text
User thread ─── write() ──→ smoltcp socket ──→ NIC driver
                           │                   ↑
                           └── poll() ←──── IRQ handler
```

## Invariants

- Network interface lock held for packet transmission
- TCP state machine lock-free on the data path
- DNS cache read-locked during lookups
- DHCP renews lease at 50% of lease lifetime
- Unix socket buffers bounded

## Dependency Breaking

```text
Original:     net → drivers (NIC) + task (CURRENT_PROCESS)
With traits:  net → vahi_types::SocketOps + vahi_types::ProcessProvider
```

## Migration Guide

1. Extract `tcp_congestion.rs` → pure algorithm, no deps
2. Extract `dns.rs` → depends on alloc + smoltcp
3. Extract `zerocopy.rs` → depends on alloc
4. Extract `dhcp.rs` → depends on smoltcp
5. Extract `unix.rs` → depends on `vahi-sync`
6. Extract `rss.rs` → depends on alloc
7. Extract `mod.rs` last
