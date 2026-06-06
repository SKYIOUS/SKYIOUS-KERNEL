pub mod dns;
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address};
use alloc::vec;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::drivers::net::{NIC, NicDevice};

lazy_static! {
    pub static ref NETWORK_INTERFACE: Mutex<Option<Interface>> = Mutex::new(None);
    pub static ref SOCKETS: Mutex<SocketSet<'static>> = Mutex::new(SocketSet::new(vec![]));
}

pub fn init() {
    let nic_lock = NIC.lock();
    if let Some(ref nic) = *nic_lock {
        let mac = nic.mac_address();
        let eth_addr = EthernetAddress(mac);

        let config = Config::new(eth_addr.into());
        
        // Setup static IP for testing
        let ip_addr = Ipv4Address::new(10, 0, 2, 15);
        let mut iface = match nic {
            NicDevice::E1000(device) => {
                let mut dev = device.lock();
                Interface::new(config, &mut *dev, Instant::from_millis(0))
            },
            NicDevice::VirtIO(device) => {
                let mut dev = device.lock();
                Interface::new(config, &mut *dev, Instant::from_millis(0))
            }
        };

        iface.update_ip_addrs(|addrs| {
            addrs.push(IpCidr::new(IpAddress::Ipv4(ip_addr), 24)).unwrap();
        });

        *NETWORK_INTERFACE.lock() = Some(iface);
        crate::println!("Network: Stack initialized with IP 10.0.2.15 (MAC: {})", eth_addr);
    } else {
        crate::println!("Network: No NIC found, stack not started.");
    }
}

pub fn poll() {
    let mut iface_lock = NETWORK_INTERFACE.lock();
    if let Some(ref mut iface) = *iface_lock {
        let mut sockets = SOCKETS.lock();
        let nic_lock = NIC.lock();
        
        if let Some(ref nic) = *nic_lock {
            match nic {
                NicDevice::E1000(device) => {
                    let mut dev = device.lock();
                    iface.poll(Instant::from_millis(0), &mut *dev, &mut sockets);
                },
                NicDevice::VirtIO(device) => {
                    let mut dev = device.lock();
                    iface.poll(Instant::from_millis(0), &mut *dev, &mut sockets);
                }
            }
        }
    }
}
