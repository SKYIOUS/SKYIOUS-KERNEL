//! # I/O APIC Module
//!
//! This module provides an interface for interacting with the I/O APIC,
//! which is responsible for routing hardware interrupts to Local APICs.

use volatile::Volatile;
use crate::memory;

/// I/O APIC MMIO register offsets (relative to the I/O APIC base).
const IOREGSEL: u32 = 0x00;
/// I/O APIC Window Register — data port selected by IOREGSEL.
const IOWIN: u32 = 0x10;

/// I/O APIC Identification Register (bits 31:24 = APIC ID).
#[allow(dead_code)]
const IOAPICID: u32 = 0x00;
/// I/O APIC Version Register — bits 23:16 of the high double-word give the
/// maximum redirection-table entry index.
const IOAPICVER: u32 = 0x01;
/// I/O APIC Arbitration ID Register.
#[allow(dead_code)]
const IOAPICARB: u32 = 0x02;
/// Redirection Table Base Offset. Each entry occupies two consecutive
/// double-words (low word, then high word).
const IOREDTBL: u32 = 0x10;

pub struct IoApic {
    base: usize,
}

impl IoApic {
    /// Create a new `IoApic` handle for the MMIO region at `base`.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `base` is the verified physical
    /// address of an I/O APIC (read from the ACPI MADT) and that the system
    /// has identity-mapped physical memory at `physical_memory_offset()`
    /// covering at least the I/O APIC's 4 KiB MMIO range.
    pub unsafe fn new(base: usize) -> Self {
        IoApic { base }
    }

    /// Compute the virtual addresses of the I/O APIC's select and window
    /// registers. The caller must write `reg` to `IOREGSEL` before reading
    /// or writing `IOWIN`.
    fn reg_ptrs(&self) -> (*mut Volatile<u32>, *mut Volatile<u32>) {
        let offset = memory::physical_memory_offset();
        let ioregsel = (offset + self.base as u64 + IOREGSEL as u64) as *mut Volatile<u32>;
        let iowin = (offset + self.base as u64 + IOWIN as u64) as *mut Volatile<u32>;
        (ioregsel, iowin)
    }

    /// Read a 32-bit I/O APIC register selected by `reg`.
    ///
    /// # Safety (per-call)
    ///
    /// The `unsafe` is localized because the MMIO window address is derived
    /// from the ACPI-provided base and is guaranteed valid for the lifetime of
    /// this `IoApic` handle. The caller of `new` upholds the aliasing and
    /// validity contract documented above.
    fn read(&self, reg: u32) -> u32 {
        let (ioregsel, iowin) = self.reg_ptrs();
        // SAFETY: pointer arithmetic stays within the 4 KiB I/O APIC MMIO
        // window (IOREGSEL and IOWIN are 0x00 and 0x10 from the base).
        unsafe {
            (*ioregsel).write(reg);
            (*iowin).read()
        }
    }

    /// Write a 32-bit value to I/O APIC register `reg`.
    fn write(&mut self, reg: u32, value: u32) {
        let (ioregsel, iowin) = self.reg_ptrs();
        // SAFETY: same derivation as `read`; `reg` indexes a valid register.
        unsafe {
            (*ioregsel).write(reg);
            (*iowin).write(value);
        }
    }

    /// Read a 64-bit redirection-table entry as `(low, high)`.
    ///
    /// Takes `&self` because this is a pure read with no side effects beyond
    /// the (benign) select-then-read MMIO transaction.
    pub fn read_redirection_entry(&self, index: u8) -> (u32, u32) {
        let low_reg = IOREDTBL + (index as u32 * 2);
        let high_reg = low_reg + 1;
        (self.read(low_reg), self.read(high_reg))
    }

    /// Maximum redirection-table index supported by this I/O APIC
    /// (per the version register, encoded as the last valid entry).
    pub fn max_redirection_entry(&self) -> u8 {
        ((self.read(IOAPICVER) >> 16) & 0xFF) as u8
    }

    /// Program a redirection-table entry.
    ///
    /// `index` selects the IRQ input line, `vector` is the target LAPIC
    /// interrupt vector, `dest_lapic_id` is the delivery destination, and the
    /// polarity/trigger/mask flags map directly to Redirection Table Entry
    /// low-double-word bits (Intel SDM Vol. 3 §10.12):
    ///   * `active_low`   → bit 13 (polarity: 1 = active-low)
    ///   * `level_triggered` → bit 15 (trigger mode: 1 = level)
    ///   * `masked`       → bit 16
    pub fn set_redirection(
        &mut self,
        index: u8,
        vector: u8,
        dest_lapic_id: u8,
        active_low: bool,
        level_triggered: bool,
        masked: bool,
    ) {
        let max = self.max_redirection_entry();
        assert!(
            index <= max,
            "I/O APIC redirection index {} exceeds max supported entry {}",
            index,
            max
        );

        let low_reg = IOREDTBL + (index as u32 * 2);
        let high_reg = low_reg + 1;

        let mut low = vector as u32;
        if active_low {
            low |= 1 << 13; // Polarity: active-low
        }
        if level_triggered {
            low |= 1 << 15; // Trigger mode: level
        }
        if masked {
            low |= 1 << 16; // Mask
        }

        #[cfg(debug_assertions)]
        crate::serial_write(&alloc::format!(
            "[IOAPIC] set_redir idx={} vec={} dest={}\n",
            index, vector, dest_lapic_id
        ));

        self.write(low_reg, low);
        self.write(high_reg, (dest_lapic_id as u32) << 24);

        #[cfg(debug_assertions)]
        {
            let (rlow, rhigh) = self.read_redirection_entry(index);
            crate::serial_write(&alloc::format!(
                "[IOAPIC] readback idx={} low=0x{:08x} high=0x{:08x} expected_low=0x{:08x}\n",
                index, rlow, rhigh, low
            ));
            crate::serial_write("[IOAPIC] done\n");
        }

        crate::apic::errata::apply_ioapic_workarounds(self, index);
    }
}

