#![allow(dead_code)]
//! VirtIO Block device.
//!
//! Emulates a virtio-blk device backed by a host file or memory buffer.
//! Uses the VirtIO MMIO transport for simplicity (no PCI required).

use alloc::vec::Vec;
use x86_64::PhysAddr;
use crate::hypervisor::devices::{VirtDevice, VirtDeviceType};

/// VirtIO block device.
pub struct VirtioBlock {
    pub capacity: u64,
    pub sector_size: u32,
    pub data: Vec<u8>,
    pub mmio_base: PhysAddr,
}

impl VirtioBlock {
    pub fn new(capacity_mb: usize, mmio_base: PhysAddr) -> Self {
        let size = capacity_mb * 1024 * 1024;
        VirtioBlock {
            capacity: size as u64,
            sector_size: 512,
            data: alloc::vec![0u8; size],
            mmio_base,
        }
    }

    fn read_sector(&self, sector: u64, data: &mut [u8]) -> bool {
        let offset = (sector * self.sector_size as u64) as usize;
        if offset + data.len() > self.data.len() {
            return false;
        }
        data.copy_from_slice(&self.data[offset..offset + data.len()]);
        true
    }

    fn write_sector(&mut self, sector: u64, data: &[u8]) -> bool {
        let offset = (sector * self.sector_size as u64) as usize;
        if offset + data.len() > self.data.len() {
            return false;
        }
        self.data[offset..offset + data.len()].copy_from_slice(data);
        true
    }
}

impl VirtDevice for VirtioBlock {
    fn device_type(&self) -> VirtDeviceType {
        VirtDeviceType::VirtioBlock
    }

    fn mmio_regions(&self) -> Vec<(PhysAddr, usize)> {
        alloc::vec![(self.mmio_base, 0x1000)]
    }

    fn port_io_ranges(&self) -> Vec<(u16, u16)> {
        Vec::new()
    }

    fn handle_mmio_read(&mut self, addr: PhysAddr, _size: u8) -> Option<u64> {
        let offset = addr.as_u64() - self.mmio_base.as_u64();
        // ponytail: minimal VirtIO MMIO register emulation
        match offset {
            0x000 => Some(0x0), // Magic value
            0x004 => Some(0x2), // Version
            0x008 => Some(0xFF), // Device ID (block = 0x02)
            0x00C => Some(0x1), // Vendor ID
            0x010 => Some(self.capacity), // Device features
            _ => Some(0),
        }
    }

    fn handle_mmio_write(&mut self, addr: PhysAddr, _size: u8, value: u64) -> bool {
        let _offset = addr.as_u64() - self.mmio_base.as_u64();
        // ponytail: queue descriptor handling and I/O request dispatch
        // add when full VirtIO transport is wired
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
        // ponytail: reset queue state
    }
}
