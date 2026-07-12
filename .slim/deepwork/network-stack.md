# TCP/IP Network Stack — Deep Work

## Context
The kernel already has a full smoltcp-based network stack (smoltcp 0.10 with proto-ipv4, proto-ipv6, socket-icmp, socket-udp, socket-tcp, socket-dhcpv4). The E1000 and VirtIO NIC drivers implement `smoltcp::phy::Device`. Socket syscalls (socket/bind/connect/listen/accept/sendto/recvfrom) are implemented for IPv4. Userspace has httpd, wget, curl, nc, echod.

## Gaps vs Requirements
| Requirement | Status |
|------------|--------|
| ARP cache | ✅ smoltcp internal |
| IP checksum | ✅ smoltcp internal |
| ICMP Echo (ping) | ✅ smoltcp socket-icmp |
| TCP state machine | ✅ smoltcp socket-tcp |
| IPv4 socket syscalls | ✅ SYS_SOCKET=41, BIND=49, CONNECT=42, LISTEN=50, ACCEPT=43, SENDTO=44, RECVFROM=45 |
| UDP socket syscalls | ✅ |
| read/write on sockets | ✅ SYS_READ/SYS_WRITE dispatch |
| close on sockets | ✅ SYS_CLOSE |
| **IPv6: SockAddrIn6 parsing** | ❌ sys_socket/bind/connect only handle AF_INET=2 |
| **IPv6: ICMPv6** | ✅ smoltcp socket-icmp covers both ICMPv4/v6 |
| **setsockopt** | ❌ not implemented |
| **IPv6 in userland** | ❌ SockAddrIn6 in libsarga, httpd/wget parse |
| **Unit tests** | ❌ no network packet tests |

## Plan
1. Phase 1: Add AF_INET6 domain handling + SockAddrIn6 in syscalls
2. Phase 2: Add setsockopt syscall (SYS_SETSOCKOPT)
3. Phase 3: IPv6 SockAddrIn6 + parsing in libsarga (userspace)
4. Phase 4: IPv6 support in httpd/wget userland tools
5. Phase 5: Network packet unit tests (dummy IP/TCP/UDP)
6. Phase 6: Documentation
