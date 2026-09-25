//! I/O APIC module — MMIO register access for interrupt routing.
//!
//! The I/O APIC (IOAPIC) receives interrupt lines from PCI/legacy devices
//! and routes them to LAPICs via the system bus. Each IOAPIC exposes:
//!
//! - **IOREGSEL** (0x00) — Register select (index)
//! - **IOWIN** (0x10) — Register window (data)
//! - **Redirection Table** — One 64-bit entry per GSI (Global System Interrupt)
//!
//! ## Redirection Entry Format
//!
//! ```text
//! 63:56  55  54:53  52  51:48  47:32  31:17  16  15  14  13  12  11  10:8  7:0
//! ┌──────┬───┬──────┬───┬──────┬──────┬──────┬────┬────┬────┬────┬────┬────┬────┐
//! │ Dest │ 0 │ DM    │ 0 │ Dest │ Mask │Trig  │ Rr │Pol │Del │ 0 │ 0 │ Mode│Vec │
//! │      │   │       │   │      │      │(L/H) │    │(L/H)│(0)│   │   │     │    │
//! └──────┴───┴──────┴───┴──────┴──────┴──────┴────┴────┴────┴────┴────┴────┴────┘
//! ```
//!
//! ## Invariants
//!
//! - MMIO base address is provided by ACPI MADT (multiple IOAPICs possible)
//! - Each redirection entry maps one GSI to one LAPIC vector
//! - Polarity and trigger mode come from ACPI Interrupt Source Overrides
//! - Mask bit must be cleared to enable the interrupt

use volatile::Volatile;

const IOREGSEL: u32 = 0x00;
const IOWIN: u32 = 0x10;
const IOAPICVER: u32 = 0x01;
const IOREDTBL: u32 = 0x10;

pub struct IoApic {
    base: usize,
}

impl IoApic {
    /// # Safety
    ///
    /// `base` must be a verified I/O APIC physical address with identity-mapped
    /// memory covering at least its 4 KiB MMIO range.
    pub unsafe fn new(base: usize) -> Self {
        IoApic { base }
    }

    fn reg_ptrs(&self) -> (*mut Volatile<u32>, *mut Volatile<u32>) {
        let offset = super::pmo();
        let ioregsel = (offset + self.base as u64 + IOREGSEL as u64) as *mut Volatile<u32>;
        let iowin = (offset + self.base as u64 + IOWIN as u64) as *mut Volatile<u32>;
        (ioregsel, iowin)
    }

    pub fn read(&self, reg: u32) -> u32 {
        let (ioregsel, iowin) = self.reg_ptrs();
        // SAFETY: IOREGSEL/IOWIN are MMIO-mapped I/O APIC registers.
        // Write selects the register, read returns its value.
        unsafe {
            (*ioregsel).write(reg);
            (*iowin).read()
        }
    }

    pub fn write(&mut self, reg: u32, value: u32) {
        let (ioregsel, iowin) = self.reg_ptrs();
        // SAFETY: IOREGSEL/IOWIN are MMIO-mapped I/O APIC registers.
        // Write selects the register, next write stores the value.
        unsafe {
            (*ioregsel).write(reg);
            (*iowin).write(value);
        }
    }

    pub fn read_redirection_entry(&self, index: u8) -> (u32, u32) {
        let low_reg = IOREDTBL + (index as u32 * 2);
        let high_reg = low_reg + 1;
        (self.read(low_reg), self.read(high_reg))
    }

    pub fn max_redirection_entry(&self) -> u8 {
        ((self.read(IOAPICVER) >> 16) & 0xFF) as u8
    }

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
            low |= 1 << 13;
        }
        if level_triggered {
            low |= 1 << 15;
        }
        if masked {
            low |= 1 << 16;
        }

        self.write(low_reg, low);
        self.write(high_reg, (dest_lapic_id as u32) << 24);

        super::errata::apply_ioapic_workarounds(self, index);
    }
}
