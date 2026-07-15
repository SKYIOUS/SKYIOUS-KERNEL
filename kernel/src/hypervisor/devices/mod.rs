//! Virtual device trait and type definitions.
//!
//! Devices are attached to guest VMs and emulate hardware for I/O,
//! paravirtualized VirtIO, and legacy device models.

use alloc::vec::Vec;
use x86_64::PhysAddr;

/// Types of virtual devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtDeviceType {
    VirtioBlock,
    VirtioNet,
    VirtioGpu,
    Uart16550,
    PicPit,
    Kvmi,
}

pub mod virtio_block;
pub mod virtio_net;
pub mod uart;
pub mod pic;

/// Trait for virtual device emulation.
pub trait VirtDevice: Send + Sync {
    fn device_type(&self) -> VirtDeviceType;
    fn mmio_regions(&self) -> Vec<(PhysAddr, usize)>;
    fn port_io_ranges(&self) -> Vec<(u16, u16)>;
    fn handle_mmio_read(&mut self, addr: PhysAddr, size: u8) -> Option<u64>;
    fn handle_mmio_write(&mut self, addr: PhysAddr, size: u8, value: u64) -> bool;
    fn handle_pio_read(&mut self, port: u16, size: u8) -> Option<u32>;
    fn handle_pio_write(&mut self, port: u16, size: u8, value: u32) -> bool;
    fn reset(&mut self);
}
