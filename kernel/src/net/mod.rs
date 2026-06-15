pub mod dns;
pub mod dhcp;
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address};
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

        iface.update_ip_addrs(|addrs| {
            addrs.push(IpCidr::new(IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 15)), 24)).unwrap();
        });
        
        // Fallback default route via QEMU user-mode gateway
        iface.routes_mut().add_default_ipv4_route(Ipv4Address::new(10, 0, 2, 2)).ok();

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
    let mut iface_lock = NETWORK_INTERFACE.lock();
    if let Some(ref mut iface) = *iface_lock {
        let mut sockets = SOCKETS.lock();
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
                                        addrs.clear();
                                        addrs.push(smoltcp::wire::IpCidr::Ipv4(config.address)).ok();
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
