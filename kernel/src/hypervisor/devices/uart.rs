//! Virtual 16550 UART.
//!
//! Emulates a 16550-compatible serial port for guest console I/O.
//! I/O ports 0x3F8–0x3FF (COM1) with IRQ 4.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use x86_64::PhysAddr;
use crate::hypervisor::devices::{VirtDevice, VirtDeviceType};

const UART_BASE: u16 = 0x3F8;
const UART_SIZE: u16 = 8;
const UART_IRQ: u8 = 4;

/// 16550 UART registers (offset from base).
const RBR: u16 = 0; // Read: Receive buffer
const THR: u16 = 0; // Write: Transmit hold
const IER: u16 = 1; // Interrupt enable
const IIR: u16 = 2; // Interrupt identification (read)
const FCR: u16 = 2; // FIFO control (write)
const LCR: u16 = 3; // Line control
const MCR: u16 = 4; // Modem control
const LSR: u16 = 5; // Line status
const MSR: u16 = 6; // Modem status

/// Virtual 16550 UART.
pub struct Uart {
    pub rx_buffer: VecDeque<u8>,
    pub tx_buffer: VecDeque<u8>,
    pub irq: u8,
    pub ier: u8,
    pub lcr: u8,
    pub mcr: u8,
    pub dlab: bool,
}

impl Uart {
    pub fn new() -> Self {
        Uart {
            rx_buffer: VecDeque::new(),
            tx_buffer: VecDeque::new(),
            irq: UART_IRQ,
            ier: 0,
            lcr: 0,
            mcr: 0,
            dlab: false,
        }
    }

    /// Inject a byte into the guest's RX buffer (as if received on serial).
    pub fn inject_byte(&mut self, byte: u8) {
        self.rx_buffer.push_back(byte);
    }

    fn read_byte(&mut self, reg: u16) -> u8 {
        match reg {
            RBR if !self.dlab => self.rx_buffer.pop_front().unwrap_or(0),
            IER if !self.dlab => self.ier,
            IIR => 0xC1, // No interrupt pending + FIFO enabled
            LCR => self.lcr,
            MCR => self.mcr,
            LSR => {
                let mut lsr = 0x60; // TX empty + TX holding empty
                if !self.rx_buffer.is_empty() {
                    lsr |= 0x01; // Data ready
                }
                lsr
            }
            MSR => 0xB0, // DCD + RI + DSR + CTS
            _ => 0,
        }
    }

    fn write_byte(&mut self, reg: u16, value: u8) {
        match reg {
            THR if !self.dlab => {
                self.tx_buffer.push_back(value);
                // Write to host serial output
                crate::serial_putc(value);
            }
            IER if !self.dlab => self.ier = value,
            FCR => { /* Ignore FIFO settings */ }
            LCR => {
                self.lcr = value;
                self.dlab = (value & 0x80) != 0;
            }
            MCR => self.mcr = value,
            _ => {}
        }
    }
}

impl VirtDevice for Uart {
    fn device_type(&self) -> VirtDeviceType {
        VirtDeviceType::Uart16550
    }

    fn mmio_regions(&self) -> Vec<(PhysAddr, usize)> {
        Vec::new()
    }

    fn port_io_ranges(&self) -> Vec<(u16, u16)> {
        alloc::vec![(UART_BASE, UART_SIZE)]
    }

    fn handle_mmio_read(&mut self, _addr: PhysAddr, _size: u8) -> Option<u64> {
        None
    }

    fn handle_mmio_write(&mut self, _addr: PhysAddr, _size: u8, _value: u64) -> bool {
        false
    }

    fn handle_pio_read(&mut self, port: u16, _size: u8) -> Option<u32> {
        if port >= UART_BASE && port < UART_BASE + UART_SIZE {
            Some(self.read_byte(port - UART_BASE) as u32)
        } else {
            None
        }
    }

    fn handle_pio_write(&mut self, port: u16, _size: u8, value: u32) -> bool {
        if port >= UART_BASE && port < UART_BASE + UART_SIZE {
            self.write_byte(port - UART_BASE, value as u8);
            true
        } else {
            false
        }
    }

    fn reset(&mut self) {
        self.rx_buffer.clear();
        self.tx_buffer.clear();
        self.ier = 0;
        self.lcr = 0;
        self.mcr = 0;
        self.dlab = false;
    }
}
