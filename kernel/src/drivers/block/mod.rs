//! Block Device Abstraction
//!
//! This module provides a generic block device trait for storage drivers.
//! Block devices are accessed in fixed-size sectors (typically 512 bytes).
//!
//! # Usage
//! Implement the `BlockDevice trait for specific storage drivers (IDE, AHCI, etc.).

pub mod partition;

// use alloc::vec::Vec;
use alloc::sync::Arc;
use spin::Mutex;
use alloc::vec::Vec;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref BLOCK_DEVICES: Mutex<Vec<Arc<Mutex<dyn BlockDevice>>>> = Mutex::new(Vec::new());
}

pub fn register_block_device(device: Arc<Mutex<dyn BlockDevice>>) {
    BLOCK_DEVICES.lock().push(device);
}
pub trait BlockDevice: Send + Sync {
    /// Reads a single sector from the device into the provided buffer.
    ///
    /// # Arguments
    /// * `sector` - The logical sector number to read
    /// * `buf` - Buffer to store the sector data (should be at least sector size)
    ///
    /// # Errors
    /// Returns `BlockDeviceError::ReadError` if the read operation fails.
    fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError>;
    
    /// Writes a single sector to the device from the provided buffer.
    ///
    /// # Arguments
    /// * `sector` - The logical sector number to write
    /// * `buf` - Buffer containing the data to write (should be at least sector size)
    ///
    /// # Errors
    /// Returns `BlockDeviceError::WriteError` if the write operation fails.
    fn write_sector(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockDeviceError>;
    
    /// Returns the total number of sectors available on this device.
    ///
    /// # Errors
    /// Returns `BlockDeviceError::DeviceError` if size cannot be determined.
    fn sector_count(&self) -> Result<u64, BlockDeviceError>;
}

/// Errors that can occur during block device operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockDeviceError {
    /// An error occurred while reading from the device
    ReadError,
    /// An error occurred while writing to the device
    WriteError,
    /// A general device error (e.g., device not ready, not found)
    DeviceError,
    /// The requested sector is out of bounds
    InvalidSector,
}
