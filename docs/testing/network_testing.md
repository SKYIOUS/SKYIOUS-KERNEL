# Network Stack Testing

The network stack is tested through unit tests, simulated environments, and QEMU networking.

## Unit Tests

Protocol-level tests validate packet handling:

```rust
#[test_case]
fn test_tcp_checksum() {
    let packet = build_test_packet();
    let computed = tcp::checksum(&packet);
    assert_eq!(computed, EXPECTED_CHECKSUM);
}

#[test_case]
fn test_arp_request_response() {
    let mut cache = ArpCache::new();
    let request = build_arp_request(MAC_A, IP_A, IP_B);
    cache.handle_packet(&request);
    assert!(cache.lookup(IP_B).is_some());
}
```

## QEMU Network Tests

Integration tests use QEMU's user-mode networking:

```bash
cargo test --test integration network
```

Tests verify:
- ARP resolution succeeds
- TCP connection establishment and teardown
- UDP send/receive correctness
- ICMP echo (ping) response
- DHCP lease acquisition
- DNS resolution

## Network Namespace Testing

For more advanced testing, QEMU can connect to a virtual network namespace:

```bash
# Create a virtual network
ip netns add skyos-test
ip link add veth0 type veth peer name veth1
ip link set veth1 netns skyos-test

# Launch QEMU connected to the namespace
cargo run --release -- --nic tap,ifname=veth0
```

## Packet Capture

Network tests can capture packets for analysis:

```bash
# Capture packets from QEMU
tcpdump -i any port 8080 -w capture.pcap
```

## Performance Testing

Throughput and latency benchmarks:
```bash
cargo test --test bench network_throughput
cargo test --test bench network_latency
```
