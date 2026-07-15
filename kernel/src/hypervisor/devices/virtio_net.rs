//! VirtIO Net device.
//!
//! Emulates a virtio-net device that forwards packets between the guest
//! and the host networking stack. Uses the VirtIO MMIO transport.

use alloc::vec::Vec;
use x86_64::PhysAddr;
use crate::hypervisor::devices::{VirtDevice, VirtDeviceType};

const MAC_ADDR: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

/// VirtIO network device.
pub struct VirtioNet {
    pub mmio_base: PhysAddr,
    pub mac: [u8; 6],
    pub rx_queue: Vec<u8>,
    pub tx_queue: Vec<u8>,
    pub link_up: bool,
}

impl VirtioNet {
    pub fn new(mmio_base: PhysAddr) -> Self {
        VirtioNet {
            mmio_base,
            mac: MAC_ADDR,
            rx_queue: Vec::new(),
            tx_queue: Vec::new(),
            link_up: true,
        }
    }

    /// Transmit a packet to the guest.
    pub fn inject_packet(&mut self, data: &[u8]) {
        self.rx_queue.extend_from_slice(data);
    }

    /// Receive a packet from the guest.
    pub fn receive_packet(&mut self) -> Option<Vec<u8>> {
        if self.tx_queue.is_empty() {
            return None;
        }
        // ponytail: proper packet framing from VirtIO descriptors
        // add when descriptor ring parsing is implemented
        Some(self.tx_queue.split_off(0))
    }
}

impl VirtDevice for VirtioNet {
    fn device_type(&self) -> VirtDeviceType {
        VirtDeviceType::VirtioNet
    }

    fn mmio_regions(&self) -> Vec<(PhysAddr, usize)> {
        alloc::vec![(self.mmio_base, 0x1000)]
    }

    fn port_io_ranges(&self) -> Vec<(u16, u16)> {
        Vec::new()
    }

    fn handle_mmio_read(&mut self, addr: PhysAddr, _size: u8) -> Option<u64> {
        let offset = addr.as_u64() - self.mmio_base.as_u64();
        match offset {
            0x000 => Some(0x74726976), // "virt" magic
            0x004 => Some(0x2),        // Version
            0x008 => Some(0x1),        // Device ID (net = 0x01)
            0x00C => Some(0x1),        // Vendor ID
            0x010 => Some(0x0),        // Device features
            _ => Some(0),
        }
    }

    fn handle_mmio_write(&mut self, addr: PhysAddr, _size: u8, value: u64) -> bool {
        let _offset = addr.as_u64() - self.mmio_base.as_u64();
        let _ = value;
        true
    }

    fn handle_pio_read(&mut self, _port: u16, _size: u8) -> Option<u32> {
        None
    }

    fn handle_pio_write(&mut self, _port: u16, _size: u8, _value: u32) -> bool {
        false
    }

    fn reset(&mut self) {
        self.rx_queue.clear();
        self.tx_queue.clear();
        self.link_up = true;
    }
}
