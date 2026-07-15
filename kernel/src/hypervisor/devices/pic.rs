//! Virtual PIC/PIT (Programmable Interrupt Controller / Timer).
//!
//! Provides legacy 8259A PIC and 8253 PIT emulation for guests that
//! do not support APIC. The virtual PIC maps guest interrupt requests
//! to the VCPU's interrupt injection mechanism.

use alloc::vec::Vec;
use x86_64::PhysAddr;
use crate::hypervisor::devices::{VirtDevice, VirtDeviceType};

const PIC1_BASE: u16 = 0x20;
const PIC2_BASE: u16 = 0xA0;
const PIT_BASE: u16 = 0x40;

/// Virtual PIC/PIT.
pub struct PicPit {
    pub pic1_imr: u8,   // Interrupt mask register (PIC1)
    pub pic2_imr: u8,
    pub pic1_irr: u8,   // Interrupt request register
    pub pic1_isr: u8,   // In-service register
    pub pic2_irr: u8,
    pub pic2_isr: u8,
    pub pit_counter: u32,
    pub pit_reload: u32,
    pub irq_pending: Vec<u8>,
}

impl PicPit {
    pub fn new() -> Self {
        PicPit {
            pic1_imr: 0xFF,
            pic2_imr: 0xFF,
            pic1_irr: 0,
            pic1_isr: 0,
            pic2_irr: 0,
            pic2_isr: 0,
            pit_counter: 0,
            pit_reload: 0xFFFF,
            irq_pending: Vec::new(),
        }
    }

    /// Raise an IRQ line (0-15).
    pub fn raise_irq(&mut self, irq: u8) {
        if irq < 8 {
            self.pic1_irr |= 1 << irq;
        } else {
            self.pic2_irr |= 1 << (irq - 8);
        }
        // ponytail: assert interrupt to VCPU
        // add when VCPU interrupt injection is wired to device model
    }

    /// Lower an IRQ line (0-15).
    pub fn lower_irq(&mut self, irq: u8) {
        if irq < 8 {
            self.pic1_irr &= !(1 << irq);
        } else {
            self.pic2_irr &= !(1 << (irq - 8));
        }
    }

    /// Read a PIC/PIT register.
    fn read_byte(&self, port: u16) -> u8 {
        match port {
            p if p == PIC1_BASE => self.pic1_irr,
            p if p == PIC1_BASE + 1 => self.pic1_imr,
            p if p == PIC2_BASE => self.pic2_irr,
            p if p == PIC2_BASE + 1 => self.pic2_imr,
            p if p == PIT_BASE => (self.pit_counter & 0xFF) as u8,
            p if p == PIT_BASE + 1 => ((self.pit_counter >> 8) & 0xFF) as u8,
            _ => 0,
        }
    }

    /// Write a PIT counter reload value.
    fn write_byte(&mut self, port: u16, value: u8) {
        match port {
            p if p == PIC1_BASE + 1 => self.pic1_imr = value,
            p if p == PIC2_BASE + 1 => self.pic2_imr = value,
            p if p == PIT_BASE => {
                self.pit_reload = (self.pit_reload & 0xFF00) | value as u32;
            }
            p if p == PIT_BASE + 1 => {
                self.pit_reload = (self.pit_reload & 0x00FF) | ((value as u32) << 8);
            }
            _ => {}
        }
    }
}

impl VirtDevice for PicPit {
    fn device_type(&self) -> VirtDeviceType {
        VirtDeviceType::PicPit
    }

    fn mmio_regions(&self) -> Vec<(PhysAddr, usize)> {
        Vec::new()
    }

    fn port_io_ranges(&self) -> Vec<(u16, u16)> {
        alloc::vec![(0x20, 2), (0xA0, 2), (0x40, 4)]
    }

    fn handle_mmio_read(&mut self, _addr: PhysAddr, _size: u8) -> Option<u64> {
        None
    }

    fn handle_mmio_write(&mut self, _addr: PhysAddr, _size: u8, _value: u64) -> bool {
        false
    }

    fn handle_pio_read(&mut self, port: u16, _size: u8) -> Option<u32> {
        Some(self.read_byte(port) as u32)
    }

    fn handle_pio_write(&mut self, port: u16, _size: u8, value: u32) -> bool {
        self.write_byte(port, value as u8);
        true
    }

    fn reset(&mut self) {
        self.pic1_imr = 0xFF;
        self.pic2_imr = 0xFF;
        self.pic1_irr = 0;
        self.pic1_isr = 0;
        self.pic2_irr = 0;
        self.pic2_isr = 0;
        self.pit_counter = 0;
        self.pit_reload = 0xFFFF;
    }
}
