# Phase 2: Networking Improvements

Phase 2 adds a complete networking stack and driver support.

## Goals

- IPv4 networking with TCP and UDP
- ARP and ICMP support
- BSD socket API
- DHCP client
- DNS resolution
- Network drivers (e1000, VirtIO-net)

## Key Milestones

1. **Ethernet driver**: Working e1000 and VirtIO-net drivers with DMA support
2. **ARP**: Address resolution protocol with caching
3. **IP routing**: IPv4 forwarding, fragmentation, and reassembly
4. **TCP**: Connection management, flow control, congestion control
5. **UDP**: Datagram delivery with checksum verification
6. **Socket API**: Full BSD socket interface including select/poll
7. **DHCP**: Automatic network configuration

## Testing

Network tests will use QEMU's user-mode networking and virtual network interfaces. A test suite will verify:
- TCP connection establishment and teardown
- Data throughput and correctness
- Packet loss and reordering resilience
- Multiple concurrent connections

## Expected Timeline

3-4 months after Phase 1 completion.
