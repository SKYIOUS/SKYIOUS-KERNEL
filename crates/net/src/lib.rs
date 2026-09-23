//! # vahi-net — Networking Stack
//!
//! TCP/UDP networking via smoltcp, DNS resolution, DHCP client,
//! Unix domain sockets, and TCP Reno congestion control.
//! ~1,927 lines across 7 source files.
//!
//! ## Module Breakdown
//!
//! | Module | Lines | Purpose |
//! |--------|-------|---------|
//! | `mod.rs` | 208 | Network interface, socket table, packet dispatch |
//! | `tcp_congestion.rs` | 221 | TCP Reno (cwnd, slow start, fast recovery) |
//! | `dns.rs` | 135 | DNS resolver with caching |
//! | `unix.rs` | 491 | Unix domain sockets (AF_UNIX) |
//! | `zerocopy.rs` | 563 | Zero-copy packet buffers |
//! | `rss.rs` | 292 | Receive-Side Scaling for multi-queue NICs |
//! | `dhcp.rs` | 17 | DHCP client |
//!
//! ## Dependency Breaking
//!
//! ```text
//! Original:     net → drivers (NIC) + task (CURRENT_PROCESS)
//! With traits:  net → vahi-types::SocketOps + vahi_types::ProcessProvider
//! ```
//!
//! ## Invariants
//!
//! - **Network interface lock** held for packet transmission
//! - **TCP state machine** must be lock-free on the data path
//! - **DNS cache** is read-locked during lookups
//! - **DHCP** renews lease at 50% of lease lifetime
//! - **Unix socket buffers** are bounded
//!
//! ## Concurrency Model
//!
//! ```text
//! User thread ─── write() ──→ smoltcp socket ──→ NIC driver
//!                              │                   ↑
//!                              └── poll() ←──── IRQ handler
//! ```
//!
//! ## Migration Guide
//!
//! 1. Extract `tcp_congestion.rs` → pure algorithm, no deps
//! 2. Extract `dns.rs` → depends on alloc + smoltcp
//! 3. Extract `zerocopy.rs` → depends on alloc
//! 4. Extract `dhcp.rs` → depends on smoltcp
//! 5. Extract `unix.rs` → depends on `vahi-sync`
//! 6. Extract `rss.rs` → depends on alloc
//! 7. Extract `mod.rs` last (socket table, interface management)

#![no_std]
#![allow(dead_code, unused_variables, unused_imports)]
#![allow(
    clippy::result_unit_err,
    clippy::missing_safety_doc,
    clippy::not_unsafe_ptr_arg_deref
)]
#![allow(clippy::manual_range_contains, clippy::manual_abs_diff)]
#![allow(clippy::unnecessary_cast, clippy::new_without_default)]
#![allow(clippy::needless_range_loop, clippy::identity_op)]
#![allow(clippy::declare_interior_mutable_const)]
#![allow(clippy::needless_bool, clippy::manual_clamp)]
#![allow(clippy::ptr_arg, clippy::useless_conversion)]
#![allow(clippy::needless_borrows_for_generic_args)]

extern crate alloc;

#[cfg(all(not(test), target_os = "none"))]
extern "Rust" {
    fn vahi_kernel_get_ticks() -> u64;
}

/// Monotonic 100Hz tick counter, provided by `vahi_kernel`.
#[cfg(all(not(test), target_os = "none"))]
pub fn get_ticks() -> u64 {
    // SAFETY: vahi_kernel::interrupts defines the Rust-ABI #[no_mangle]
    // `vahi_kernel_get_ticks`; the kernel binary always links it.
    unsafe { vahi_kernel_get_ticks() }
}

/// Host builds/tests have no kernel linked — return 0 as before.
#[cfg(not(all(not(test), target_os = "none")))]
pub fn get_ticks() -> u64 {
    0
}

// In test mode on non-no_std targets, use spin::Mutex to avoid IrqSafeMutex's cli/sti.
#[cfg(not(all(test, not(target_os = "none"))))]
pub(crate) type NetMutex<T> = vahi_sync::IrqSafeMutex<T>;
#[cfg(all(test, not(target_os = "none")))]
pub(crate) type NetMutex<T> = spin::Mutex<T>;

pub mod congestion;
pub mod dhcp;
pub mod dns;
pub mod rss;
pub mod socket;
pub mod tcp_congestion;
pub mod unix;
pub mod zerocopy;

use core::sync::atomic::{AtomicBool, Ordering};

// ─── Network Interface Trait ────────────────────────────────────────

/// Abstract network interface for driver integration.
pub trait NetInterface: Send + Sync {
    /// Get the hardware MAC address.
    fn mac_address(&self) -> [u8; 6];

    /// Get the current IPv4 address.
    fn ipv4_address(&self) -> Option<[u8; 4]>;

    /// Transmit an Ethernet frame.
    fn transmit(&self, frame: &[u8]) -> Result<(), i32>;

    /// Check if the link is up.
    fn is_link_up(&self) -> bool;

    /// Get the MTU.
    fn mtu(&self) -> u16;
}

// ─── Socket Address Types ───────────────────────────────────────────

/// IPv4 socket address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocketAddrV4 {
    pub addr: [u8; 4],
    pub port: u16,
}

/// Socket address family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressFamily {
    Unix = 1,
    IPv4 = 2,
    IPv6 = 10,
}

/// Generic socket address.
#[derive(Debug, Clone, Copy)]
pub enum SocketAddr {
    Unix(SocketAddrUnix),
    V4(SocketAddrV4),
}

/// Unix domain socket address.
#[derive(Debug, Clone, Copy)]
pub struct SocketAddrUnix {
    pub path: [u8; 108],
    pub len: u8,
}

// ─── TCP State ──────────────────────────────────────────────────────

/// TCP connection states (RFC 793).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
    Closed,
}

// ─── Network Initialization ─────────────────────────────────────────

static NET_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Check if the network stack is initialized.
pub fn is_initialized() -> bool {
    NET_INITIALIZED.load(Ordering::Acquire)
}

/// Mark the network stack as initialized.
pub fn set_initialized() {
    NET_INITIALIZED.store(true, Ordering::Release);
}
use crate::NetMutex as Mutex;
use alloc::vec;
use lazy_static::lazy_static;
use smoltcp::iface::SocketHandle;
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::socket::Socket;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address, Ipv6Address};
use vahi_drivers::net::{NicDevice, NIC};

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
        let now = Instant::from_millis((crate::get_ticks() * 10) as i64);

        let mut iface = match nic {
            NicDevice::E1000(device) => {
                let mut dev = device.lock();
                Interface::new(config, &mut *dev, now)
            }
            NicDevice::VirtIO(device) => {
                let mut dev = device.lock();
                Interface::new(config, &mut *dev, now)
            }
        };

        // ponytail: IFACE_MAX_ADDR_COUNT now 4 (set via SMOLTCP_IFACE_MAX_ADDR_COUNT env var)
        iface.update_ip_addrs(|addrs| {
            addrs
                .push(IpCidr::new(
                    IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 15)),
                    24,
                ))
                .ok();
            // IPv6 loopback
            addrs
                .push(IpCidr::new(IpAddress::Ipv6(Ipv6Address::LOOPBACK), 128))
                .ok();
            // IPv6 link-local from MAC (modified EUI-64)
            let eui64 = mac_to_eui64(&mac);
            let ll = Ipv6Address::new(
                0xfe80,
                0,
                0,
                0,
                (eui64[0] as u16) << 8 | eui64[1] as u16,
                (eui64[2] as u16) << 8 | eui64[3] as u16,
                (eui64[4] as u16) << 8 | eui64[5] as u16,
                (eui64[6] as u16) << 8 | eui64[7] as u16,
            );
            addrs.push(IpCidr::new(IpAddress::Ipv6(ll), 128)).ok();
        });

        // Fallback default routes
        iface
            .routes_mut()
            .add_default_ipv4_route(Ipv4Address::new(10, 0, 2, 2))
            .ok();
        // ponytail: link-local only for IPv6; no global IPv6 gateway expected in QEMU user mode

        let mut sockets = SOCKETS.lock();
        let dhcp_socket = dhcp::create_socket();
        let dhcp_handle = sockets.add(dhcp_socket);
        *DHCP_HANDLE.lock() = Some(dhcp_handle);

        *NETWORK_INTERFACE.lock() = Some(iface);
        // println removed during extraction
    } else {
        // println removed during extraction
    }
}

pub fn poll() {
    let mut sockets = SOCKETS.lock();
    let mut iface_lock = NETWORK_INTERFACE.lock();
    if let Some(ref mut iface) = *iface_lock {
        let nic_lock = NIC.lock();
        let now = Instant::from_millis((crate::get_ticks() * 10) as i64);

        if let Some(ref nic) = *nic_lock {
            match nic {
                NicDevice::E1000(device) => {
                    let mut dev = device.lock();
                    iface.poll(now, &mut *dev, &mut sockets);
                }
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
                                        let ipv6: alloc::vec::Vec<IpCidr> = addrs
                                            .iter()
                                            .filter(|a| matches!(a, IpCidr::Ipv6(_)))
                                            .cloned()
                                            .collect();
                                        addrs.clear();
                                        addrs
                                            .push(smoltcp::wire::IpCidr::Ipv4(config.address))
                                            .ok();
                                        for a in ipv6 {
                                            addrs.push(a).ok();
                                        }
                                    });
                                    // serial_write removed during extraction
                                    // serial_write removed during extraction
                                    // serial_write removed during extraction
                                    if let Some(router) = config.router {
                                        iface.routes_mut().add_default_ipv4_route(router).ok();
                                        // serial_write removed during extraction
                                        // serial_write removed during extraction
                                        // serial_write removed during extraction
                                    }
                                    let mut dns = crate::dhcp::DHCP_DNS_SERVERS.lock();
                                    dns.clear();
                                    for server in config.dns_servers.iter() {
                                        dns.push(*server);
                                    }
                                    if !dns.is_empty() {
                                        // serial_write removed during extraction
                                        for s in dns.iter() {
                                            // serial_write removed during extraction
                                            // serial_write removed during extraction
                                        }
                                        // serial_write removed during extraction
                                    }
                                }
                                Event::Deconfigured => {
                                    // serial_write removed during extraction
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
