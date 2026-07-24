pub mod dns;
pub mod dhcp;
pub mod unix;
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address, Ipv6Address};
use alloc::vec;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::drivers::net::{NIC, NicDevice};
use smoltcp::iface::SocketHandle;
use smoltcp::socket::Socket;

lazy_static! {
    pub static ref NETWORK_INTERFACE: Mutex<Option<Interface>> = Mutex::new(None);
    pub static ref SOCKETS: Mutex<SocketSet<'static>> = Mutex::new(SocketSet::new(vec![]));
    static ref DHCP_HANDLE: Mutex<Option<SocketHandle>> = Mutex::new(None);
}

fn mac_to_eui64(mac: &[u8; 6]) -> [u8; 8] {
    let mut eui64 = [0u8; 8];
    eui64[0] = mac[0] ^ 0x02; // flip U/L bit
    eui64[1] = mac[1];
    eui64[2] = mac[2];
    eui64[3] = 0xFF;
    eui64[4] = 0xFE;
    eui64[5] = mac[3];
    eui64[6] = mac[4];
    eui64[7] = mac[5];
    eui64
}

pub fn init() {
    let nic_lock = NIC.lock();
    if let Some(ref nic) = *nic_lock {
        let mac = nic.mac_address();
        let eth_addr = EthernetAddress(mac);

        let config = Config::new(eth_addr.into());
        let now = Instant::from_millis((crate::interrupts::get_ticks() * 10) as i64);

        let mut iface = match nic {
            NicDevice::E1000(device) => {
                let mut dev = device.lock();
                Interface::new(config, &mut *dev, now)
            },
            NicDevice::VirtIO(device) => {
                let mut dev = device.lock();
                Interface::new(config, &mut *dev, now)
            }
        };

        // ponytail: IFACE_MAX_ADDR_COUNT now 4 (set via SMOLTCP_IFACE_MAX_ADDR_COUNT env var)
        iface.update_ip_addrs(|addrs| {
            addrs.push(IpCidr::new(IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 15)), 24)).unwrap();
            // IPv6 loopback
            addrs.push(IpCidr::new(IpAddress::Ipv6(Ipv6Address::LOOPBACK), 128)).unwrap();
            // IPv6 link-local from MAC (modified EUI-64)
            let eui64 = mac_to_eui64(&mac);
            let ll = Ipv6Address::new(
                0xfe80, 0, 0, 0,
                (eui64[0] as u16) << 8 | eui64[1] as u16,
                (eui64[2] as u16) << 8 | eui64[3] as u16,
                (eui64[4] as u16) << 8 | eui64[5] as u16,
                (eui64[6] as u16) << 8 | eui64[7] as u16,
            );
            addrs.push(IpCidr::new(IpAddress::Ipv6(ll), 128)).ok();
        });
        
        // Fallback default routes
        iface.routes_mut().add_default_ipv4_route(Ipv4Address::new(10, 0, 2, 2)).ok();
        // ponytail: link-local only for IPv6; no global IPv6 gateway expected in QEMU user mode

        let mut sockets = SOCKETS.lock();
        let dhcp_socket = dhcp::create_socket();
        let dhcp_handle = sockets.add(dhcp_socket);
        *DHCP_HANDLE.lock() = Some(dhcp_handle);

        *NETWORK_INTERFACE.lock() = Some(iface);
        crate::println!("Network: Stack initialized with DHCP (fallback IP 10.0.2.15, MAC: {})", eth_addr);
    } else {
        crate::println!("Network: No NIC found, stack not started.");
    }
}

pub fn poll() {
    let mut sockets = SOCKETS.lock();
    let mut iface_lock = NETWORK_INTERFACE.lock();
    if let Some(ref mut iface) = *iface_lock {
        let nic_lock = NIC.lock();
        let now = Instant::from_millis((crate::interrupts::get_ticks() * 10) as i64);

        if let Some(ref nic) = *nic_lock {
            match nic {
                NicDevice::E1000(device) => {
                    let mut dev = device.lock();
                    iface.poll(now, &mut *dev, &mut sockets);
                },
                NicDevice::VirtIO(device) => {
                    let mut dev = device.lock();
                    iface.poll(now, &mut *dev, &mut sockets);
                }
            }
        }

        let dhcp_handle = *DHCP_HANDLE.lock();
        if let Some(handle) = dhcp_handle {
            for (h, socket) in sockets.iter_mut() {
                if h == handle {
                    if let Socket::Dhcpv4(ref mut dhcp) = socket {
                        while let Some(event) = dhcp.poll() {
                            use smoltcp::socket::dhcpv4::Event;
                            match event {
                                Event::Configured(config) => {
                                    iface.update_ip_addrs(|addrs| {
                                        // Keep IPv6 addresses, replace only IPv4
                                        let ipv6: alloc::vec::Vec<IpCidr> = addrs.iter().filter(|a| matches!(a, IpCidr::Ipv6(_))).cloned().collect();
                                        addrs.clear();
                                        addrs.push(smoltcp::wire::IpCidr::Ipv4(config.address)).ok();
                                        for a in ipv6 {
                                            addrs.push(a).ok();
                                        }
                                    });
                                    crate::serial_write("[DHCP] configured IP: ");
                                    crate::serial_write(&alloc::format!("{}", config.address));
                                    crate::serial_write("\n");
                                    if let Some(router) = config.router {
                                        iface.routes_mut().add_default_ipv4_route(router).ok();
                                        crate::serial_write("[DHCP] gateway: ");
                                        crate::serial_write(&alloc::format!("{}", router));
                                        crate::serial_write("\n");
                                    }
                                    let mut dns = crate::net::dhcp::DHCP_DNS_SERVERS.lock();
                                    dns.clear();
                                    for server in config.dns_servers.iter() {
                                        dns.push(*server);
                                    }
                                    if !dns.is_empty() {
                                        crate::serial_write("[DHCP] DNS servers:");
                                        for s in dns.iter() {
                                            crate::serial_write(" ");
                                            crate::serial_write(&alloc::format!("{}", s));
                                        }
                                        crate::serial_write("\n");
                                    }
                                }
                                Event::Deconfigured => {
                                    crate::serial_write("[DHCP] lease lost\n");
                                }
                            }
                        }
                    }
                    break;
                }
            }
        }
    }
}

// ─── SocketObject: wraps smoltcp SocketHandle as a KernelObject ─────

use alloc::sync::Arc;
use crate::objects::{KernelObject, ObjectHeader, ObjectTypeId, security::SecurityDescriptor};

#[allow(dead_code)]
pub struct SocketObject {
    pub header: ObjectHeader,
    pub handle: SocketHandle,
    pub socket_type: crate::task::process::SocketType,
}

#[allow(dead_code)]
impl SocketObject {
    pub fn new(handle: SocketHandle, socket_type: crate::task::process::SocketType) -> Arc<Self> {
        Arc::new(SocketObject {
            header: ObjectHeader::new(ObjectTypeId(6), SecurityDescriptor::default_socket()),
            handle,
            socket_type,
        })
    }
}

impl KernelObject for SocketObject {
    fn header(&self) -> &ObjectHeader { &self.header }

    fn poll_readable(&self) -> bool {
        let sockets = SOCKETS.lock();
        for (h, socket) in sockets.iter() {
            if h == self.handle {
                use smoltcp::socket::Socket;
                if let Socket::Tcp(ref tcp) = socket { return tcp.may_recv(); }
            }
        }
        false
    }

    fn poll_writable(&self) -> bool {
        let sockets = SOCKETS.lock();
        for (h, socket) in sockets.iter() {
            if h == self.handle {
                use smoltcp::socket::Socket;
                if let Socket::Tcp(ref tcp) = socket { return tcp.may_send(); }
            }
        }
        true
    }
}
