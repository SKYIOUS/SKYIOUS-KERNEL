# Socket API

## Syscalls

| #  | Name        | Signature                                                           |
|----|-------------|---------------------------------------------------------------------|
| 41 | `socket`    | `int socket(long domain, long type, long protocol)`                |
| 43 | `accept`    | `int accept(int fd, struct sockaddr *addr, socklen_t *addrlen)`    |
| 42 | `connect`   | `int connect(int fd, struct sockaddr *addr, socklen_t addrlen)`    |
| 49 | `bind`      | `int bind(int fd, struct sockaddr *addr, socklen_t addrlen)`       |
| 50 | `listen`    | `int listen(int fd, int backlog)`                                  |
| 44 | `sendto`    | `ssize_t sendto(int fd, void *buf, size_t len, struct sockaddr *dest_addr, socklen_t addrlen)` |
| 45 | `recvfrom`  | `ssize_t recvfrom(int fd, void *buf, size_t len, struct sockaddr *src_addr, socklen_t *addrlen)` |
| 54 | `setsockopt`| `int setsockopt(int fd, int level, int optname, void *optval, socklen_t optlen)` |

## Domains / Families

| Constant | Value |
|----------|-------|
| AF_INET  | 2     |
| AF_INET6 | 10    |

## `sockaddr_in` (AF_INET, 16 bytes)

```
offset  size  field
0       2     sin_family = 2 (LE u16)
2       2     sin_port (BE u16)
4       4     sin_addr (IPv4, 4 bytes)
8       8     sin_zero (padding)
```

## `sockaddr_in6` (AF_INET6, 28 bytes)

```
offset  size  field
0       2     sin6_family = 10 (LE u16)
2       2     sin6_port (BE u16)
4       4     sin6_flowinfo (BE u32)
8       16    sin6_addr (IPv6, 16 bytes)
24      4     sin6_scope_id (LE u32)
```

## `setsockopt` supported options

| Level       | Option        | Value |
|-------------|---------------|-------|
| SOL_SOCKET=1| SO_RCVTIMEO   | 20    |
| SOL_SOCKET=1| SO_SNDTIMEO   | 21    |
| IPPROTO_TCP=6 | TCP_NODELAY | 1     |

Timeouts are accepted but unused (sockets are non-blocking in the kernel).

## Implementation notes

- Backed by smoltcp 0.10 with `proto-ipv4`, `proto-ipv6`, `socket-udp`, `socket-tcp`
- UDP sockets: `bind()`, `sendto()`, `recvfrom()`
- TCP sockets: `bind()`, `connect()`, `listen()`, `accept()`, `read()`, `write()`
- ICMP/Ping uses a dedicated raw socket via `socket-icmp`
- DHCP uses `socket-dhcpv4` on the management interface
