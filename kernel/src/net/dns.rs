use alloc::vec::Vec;
use alloc::vec;
use crate::net::SOCKETS;
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address};
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

pub fn resolve_hostname(name: &str) -> Option<IpAddress> {
    let dns_servers = if !crate::net::dhcp::DHCP_DNS_SERVERS.lock().is_empty() {
        let servers: Vec<IpAddress> = crate::net::dhcp::DHCP_DNS_SERVERS.lock().iter().map(|&ip| IpAddress::Ipv4(ip)).collect();
        servers
    } else {
        vec![IpAddress::Ipv4(Ipv4Address::new(8, 8, 8, 8))]
    };

    let mut sockets = SOCKETS.lock();
    
    // Create buffers for UDP
    let rx_buffer = udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 1], vec![0; 512]);
    let tx_buffer = udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 1], vec![0; 512]);
    let mut socket = udp::Socket::new(rx_buffer, tx_buffer);
    
    // Local port for DNS query
    let local_port = 54321;
    let dns_port = 53;

    if socket.bind(local_port).is_err() {
        return None;
    }

    // Prepare query
    let mut query = Vec::new();
    let header = DnsHeader::new(0x1234);
    let header_bytes: [u8; 12] = unsafe { core::mem::transmute(header) };
    query.extend_from_slice(&header_bytes);
    query.extend_from_slice(&encode_name(name));
    query.extend_from_slice(&1u16.to_be_bytes()); // Type A
    query.extend_from_slice(&1u16.to_be_bytes()); // Class IN

    // Send query
    for dns_server in &dns_servers {
        let endpoint = IpEndpoint::new(*dns_server, dns_port);
        if socket.send_slice(&query, endpoint).is_ok() {
            break;
        }
    }
    if !socket.can_send() {
        return None;
    }

    let socket_handle = sockets.add(socket);
    
    // Poll for a while to get response
    // In a real kernel, we'd wait for an interrupt or use a timeout
    for _ in 0..100 {
        drop(sockets); // Release lock for poll
        crate::net::poll();
        sockets = SOCKETS.lock();
        
        let socket = sockets.get_mut::<udp::Socket>(socket_handle);
        if socket.can_recv() {
            let mut buf = [0u8; 512];
            if let Ok((n, _)) = socket.recv_slice(&mut buf) {
                // Parse response (minimal)
                if n < 12 { continue; }
                let response_header: DnsHeader = unsafe { core::ptr::read(buf.as_ptr() as *const DnsHeader) };
                let ancount = u16::from_be(response_header.ancount);
                
                if ancount > 0 {
                    // Skip header and question
                    let mut pos = 12;
                    // Skip name
                    while buf[pos] != 0 {
                        pos += (buf[pos] as usize) + 1;
                    }
                    pos += 5; // null byte + type(2) + class(2)
                    
                    // Parse first answer
                    // Answer: Name(2 or more), Type(2), Class(2), TTL(4), RDLength(2), RData(RDLength)
                    // We expect a pointer (0xC0XX) for name
                    if buf[pos] & 0xC0 == 0xC0 {
                        pos += 2;
                    } else {
                        while buf[pos] != 0 {
                            pos += (buf[pos] as usize) + 1;
                        }
                        pos += 1;
                    }
                    
                    let atype = u16::from_be_bytes([buf[pos], buf[pos+1]]);
                    pos += 2;
                    let _aclass = u16::from_be_bytes([buf[pos], buf[pos+1]]);
                    pos += 2;
                    pos += 4; // TTL
                    let rdlength = u16::from_be_bytes([buf[pos], buf[pos+1]]);
                    pos += 2;
                    
                    if atype == 1 && rdlength == 4 { // Type A
                        let ip = Ipv4Address::new(buf[pos], buf[pos+1], buf[pos+2], buf[pos+3]);
                        sockets.remove(socket_handle);
                        return Some(IpAddress::Ipv4(ip));
                    }
                }
            }
        }
        // Small delay
        for _ in 0..100000 { unsafe { core::arch::asm!("nop"); } }
    }

    sockets.remove(socket_handle);
    None
}
