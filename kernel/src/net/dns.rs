use alloc::vec::Vec;
use alloc::vec;
use crate::net::SOCKETS;
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address, Ipv6Address};
use smoltcp::socket::udp;

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct DnsHeader {
    pub id: u16,
    pub flags: u16,
    pub qdcount: u16,
    pub ancount: u16,
    pub nscount: u16,
    pub arcount: u16,
}

impl DnsHeader {
    pub fn new(id: u16) -> Self {
        Self {
            id: id.to_be(),
            flags: 0x0100u16.to_be(), // Standard query with recursion desired
            qdcount: 1u16.to_be(),
            ancount: 0,
            nscount: 0,
            arcount: 0,
        }
    }
}

pub fn encode_name(name: &str) -> Vec<u8> {
    let mut encoded = Vec::new();
    for label in name.split('.') {
        encoded.push(label.len() as u8);
        encoded.extend_from_slice(label.as_bytes());
    }
    encoded.push(0);
    encoded
}

fn do_query(
    name: &str, query_type: u16, base_port: u16,
    dns_servers: &[IpAddress], dns_port: u16,
) -> Option<IpAddress> {
    let mut q = Vec::new();
    let hdr = DnsHeader::new(0x1234);
    let hdr_bytes: [u8; 12] = unsafe { core::mem::transmute(hdr) };
    q.extend_from_slice(&hdr_bytes);
    q.extend_from_slice(&encode_name(name));
    q.extend_from_slice(&query_type.to_be_bytes());
    q.extend_from_slice(&1u16.to_be_bytes());

    let rx = udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 1], vec![0; 512]);
    let tx = udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 1], vec![0; 512]);
    let mut sock = udp::Socket::new(rx, tx);
    sock.bind(base_port).ok()?;

    for dns_server in dns_servers {
        let endpoint = IpEndpoint::new(*dns_server, dns_port);
        if sock.send_slice(&q, endpoint).is_ok() {
            break;
        }
    }
    if !sock.can_send() {
        return None;
    }

    let mut sockets = SOCKETS.lock();
    let handle = sockets.add(sock);
    drop(sockets);

    for _ in 0..100 {
        crate::net::poll();
        let mut sockets = SOCKETS.lock();
        let s = sockets.get_mut::<udp::Socket>(handle);
        if s.can_recv() {
            let mut buf = [0u8; 512];
            let n = s.recv_slice(&mut buf).ok()?.0;
            if n < 12 || n > 512 { continue; }
            let rh: DnsHeader = unsafe { core::ptr::read(buf.as_ptr() as *const DnsHeader) };
            let ancount = u16::from_be(rh.ancount);
            if ancount > 0 {
                let mut pos = 12usize;
                loop {
                    if pos >= n { break; }
                    if buf[pos] == 0 { pos += 1; break; }
                    let label_len = buf[pos] as usize;
                    if pos + 1 + label_len > n { break; }
                    if buf[pos] & 0xC0 == 0xC0 { pos += 2; break; }
                    pos += 1 + label_len;
                }
                if pos + 10 > n { continue; }
                let atype = u16::from_be_bytes([buf[pos], buf[pos+1]]);
                pos += 2;
                let _aclass = u16::from_be_bytes([buf[pos], buf[pos+1]]);
                pos += 2;
                let _ttl_b = [buf[pos], buf[pos+1], buf[pos+2], buf[pos+3]];
                pos += 4;
                if pos + 2 > n { continue; }
                let rdlen = u16::from_be_bytes([buf[pos], buf[pos+1]]);
                pos += 2;
                if pos + rdlen as usize > n { continue; }
                if atype == 28 && rdlen == 16 {
                    let mut bytes = [0u8; 16];
                    bytes.copy_from_slice(&buf[pos..pos+16]);
                    let ip = Ipv6Address::from_bytes(&bytes);
                    sockets.remove(handle);
                    return Some(IpAddress::Ipv6(ip));
                }
                if atype == 1 && rdlen == 4 {
                    let ip = Ipv4Address::new(buf[pos], buf[pos+1], buf[pos+2], buf[pos+3]);
                    sockets.remove(handle);
                    return Some(IpAddress::Ipv4(ip));
                }
            }
        }
        drop(sockets);
        for _ in 0..100000 { unsafe { core::arch::asm!("nop"); } }
    }
    let mut sockets = SOCKETS.lock();
    sockets.remove(handle);
    None
}

pub fn resolve_hostname(name: &str) -> Option<IpAddress> {
    let dns_servers = if !crate::net::dhcp::DHCP_DNS_SERVERS.lock().is_empty() {
        crate::net::dhcp::DHCP_DNS_SERVERS.lock().iter().map(|&ip| IpAddress::Ipv4(ip)).collect::<Vec<_>>()
    } else {
        vec![IpAddress::Ipv4(Ipv4Address::new(8, 8, 8, 8))]
    };

    // Try AAAA first, then A
    do_query(name, 28, 54328, &dns_servers, 53)
        .or_else(|| do_query(name, 1, 54321, &dns_servers, 53))
}
