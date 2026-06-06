//! # I/O APIC Module
//!
//! This module provides an interface for interacting with the I/O APIC,
//! which is responsible for routing hardware interrupts to Local APICs.

use volatile::Volatile;
use crate::memory;

/// I/O APIC Register Offsets
const IOREGSEL: u32 = 0x00;
const IOWIN: u32 = 0x10;

/// I/O APIC Identification Register
const _IOAPICID: u32 = 0x00;
/// I/O APIC Version Register
const _IOAPICVER: u32 = 0x01;
/// I/O APIC Arbitration ID Register
const _IOAPICARB: u32 = 0x02;
/// Redirection Table Base Offset
const _IOREDTBL: u32 = 0x10;

pub struct IoApic {
    base: usize,
}

impl IoApic {
    pub unsafe fn new(base: usize) -> Self {
        IoApic { base }
    }

        fn _read(&self, reg: u32) -> u32 {
        let offset = *memory::PHYSICAL_MEMORY_OFFSET.get().unwrap();
        let ioregsel = (offset + self.base as u64 + IOREGSEL as u64) as *mut Volatile<u32>;
        let iowin = (offset + self.base as u64 + IOWIN as u64) as *mut Volatile<u32>;

        unsafe {
            (*ioregsel).write(reg);
            (*iowin).read()
        }
    }

    fn write(&mut self, reg: u32, value: u32) {
        let offset = *memory::PHYSICAL_MEMORY_OFFSET.get().unwrap();
        let ioregsel = (offset + self.base as u64 + IOREGSEL as u64) as *mut Volatile<u32>;
        let iowin = (offset + self.base as u64 + IOWIN as u64) as *mut Volatile<u32>;

        unsafe {
            (*ioregsel).write(reg);
            (*iowin).write(value);
        }
    }

    pub fn _max_redirection_entry(&self) -> u8 {
        ((self._read(_IOAPICVER) >> 16) & 0xFF) as u8
    }

    pub fn set_redirection(&mut self, index: u8, vector: u8, dest_lapic_id: u8, masked: bool) {
        let low_reg = _IOREDTBL + (index as u32 * 2);
        let high_reg = low_reg + 1;

        let mut low = vector as u32;
        if masked {
            low |= 1 << 16; // Mask bit
        }

        crate::serial_write(&alloc::format!("[IOAPIC] set_redir idx={} vec={} dest={}\n", index, vector, dest_lapic_id));
        self.write(low_reg, low);
        self.write(high_reg, (dest_lapic_id as u32) << 24);
        crate::serial_write("[IOAPIC] done\n");
    }
}
