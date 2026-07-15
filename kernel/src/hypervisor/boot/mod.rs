//! Guest boot protocol support.
//!
//! Different operating systems have different boot conventions.
//! This module implements boot protocol setup for supported guest OS types.

pub mod linux;
pub mod skyos;

use alloc::vec::Vec;

/// Result of boot setup.
pub struct BootConfig {
    pub entry_point: u64,
    pub boot_data: Vec<u8>,
    pub cpu_count: usize,
}

/// Supported boot protocols.
pub enum BootProtocol {
    Linux32Bit,
    Linux64Bit,
    LinuxEfiStub,
    SkyOsDirect,
    BareMetal,
}
