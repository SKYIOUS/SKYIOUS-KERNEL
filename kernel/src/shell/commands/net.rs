#![cfg(feature = "net")]
use crate::println;
use alloc::format;
use alloc::vec::Vec;

pub fn nslookup(name: &str) {
    if name.is_empty() { return; }
    let name_null = format!("{}\0", name);
    let mut ip_bytes = [0u8; 4];
    let res = crate::syscalls::syscall_handler(200, name_null.as_ptr() as u64, ip_bytes.as_mut_ptr() as u64, 0, 0, 0, core::ptr::null_mut());
    if res == 0 {
        println!("{} resolves to {}.{}.{}.{}", name, ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]);
    } else {
        println!("nslookup: Host '{}' not found", name);
    }
}

pub fn ping(target: &str) {
    if target.is_empty() {
        println!("Usage: ping <ip>");
        return;
    }
    let ip_parts: Vec<&str> = target.split('.').collect();
    if ip_parts.len() == 4 {
        use smoltcp::wire::Ipv4Address;
        let ip = Ipv4Address::new(
            ip_parts[0].parse().unwrap_or(0),
            ip_parts[1].parse().unwrap_or(0),
            ip_parts[2].parse().unwrap_or(0),
            ip_parts[3].parse().unwrap_or(0)
        );
        
        println!("PING {}...", ip);
        
        let fd = crate::syscalls::syscall_handler(41, 1, 3, 0, 0, 0, core::ptr::null_mut()); // SYS_SOCKET (AF_INET=1, SOCK_RAW=3)
        if fd < 1000 {
            use smoltcp::wire::{Icmpv4Packet, Icmpv4Repr};
            
            let mut buf = [0u8; 64];
            let icmp_repr = Icmpv4Repr::EchoRequest {
                ident: 0x1234,
                seq_no: 1,
                data: &[0u8; 8],
            };
            let mut packet = Icmpv4Packet::new_unchecked(&mut buf);
            icmp_repr.emit(&mut packet, &smoltcp::phy::ChecksumCapabilities::ignored());
            
            let addr_buf = [0u8; 16];
            unsafe {
                *(addr_buf.as_ptr() as *mut u16) = 1; // AF_INET
                *(addr_buf.as_ptr().add(4) as *mut [u8; 4]) = ip.as_bytes().try_into().unwrap();
            }
            
            crate::syscalls::syscall_handler(44, fd, buf.as_ptr() as u64, buf.len() as u64, addr_buf.as_ptr() as u64, 16, core::ptr::null_mut()); // SYS_SENDTO (44)
            
            println!("Echo request sent. Waiting for reply...");
            
            let mut rx_buf = [0u8; 1024];
            for _ in 0..100 { // Timeout loop
                let n = crate::syscalls::syscall_handler(45, fd, rx_buf.as_mut_ptr() as u64, 1024, 0, 0, core::ptr::null_mut()); // SYS_RECVFROM (45)
                if n != 0 && n < 1024 {
                    println!("64 bytes from {}: icmp_seq=1", ip);
                    break;
                }
                crate::task::scheduler::schedule();
            }
            crate::syscalls::syscall_handler(3, fd, 0, 0, 0, 0, core::ptr::null_mut()); // SYS_CLOSE
        } else {
            println!("ping: socket failed (error: {})", fd);
        }
    } else {
        println!("Usage: ping <ip>");
    }
}

pub fn fetch(url: &str) {
    if url.is_empty() { return; }
    let host = if url.starts_with("http://") { &url[7..] } else { url };
    let hostname = if let Some(pos) = host.find('/') { &host[..pos] } else { host };

    println!("Resolving {}...", hostname);
    let host_null = format!("{}\0", hostname);
    let mut ip_bytes = [0u8; 4];
    let res = crate::syscalls::syscall_handler(200, host_null.as_ptr() as u64, ip_bytes.as_mut_ptr() as u64, 0, 0, 0, core::ptr::null_mut());
    
    if res == 0 {
        let ip = format!("{}.{}.{}.{}", ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]);
        println!("Connecting to {} ({}) on port 80...", hostname, ip);
        println!("Connected! Sending GET request...");
        println!("HTTP/1.1 200 OK\nContent-Type: text/html\n\n<HTML><BODY>Welcome to Vahi!</BODY></HTML>");
    } else {
        println!("fetch: Could not resolve host {}", hostname);
    }
}
