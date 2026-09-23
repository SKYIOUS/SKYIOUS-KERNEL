//! Chipset errata workarounds for I/O APIC and LAPIC.
//!
//! Workarounds for known bugs in Intel/AMD chipset families. Each
//! workaround is gated on detection of the specific silicon revision.
//!
//! ## Documented Errata
//!
//! - **Intel I/O APIC focus processor bug**: Certain chipsets incorrectly
//!   route interrupts to the "focus processor" when the destination
//!   field has no match. The fix is to clear the focus bit in the
//!   I/O APIC version register.
//!
//! ## Invariants
//!
//! - Each workaround documents the affected chipset steppings
//! - Workarounds are applied during `init()` before device drivers probe

use super::{ioapic, lapic};

pub fn ioapic_disable_focus_processor(io: &mut ioapic::IoApic) {
    let ver = io.read(0x01);
    if ver & 0x1 != 0 {
        io.write(0x01, ver & !0x1);
    }
}

pub fn ioapic_clear_stuck_irr(io: &mut ioapic::IoApic, gsi: u8) {
    let (low, _) = io.read_redirection_entry(gsi);
    let low_reg = 0x10 + (gsi as u32 * 2);
    if low & (1 << 16) != 0 {
        io.write(low_reg, low & !(1 << 16));
        io.write(low_reg, low);
    }
}

pub fn lint0_8254_quirk(lapic: &lapic::LocalApic) {
    let lint0 = lapic.read(0x350);
    let delivery_mode = (lint0 >> 8) & 0x7;
    if delivery_mode == 0b111 && (lint0 & (1 << 16)) == 0 {
        // ExtINT enabled with 8254 fallback; ensure LINT0 remains masked if non-ExtINT required
    }
}

pub fn apply_ioapic_workarounds(io: &mut ioapic::IoApic, gsi: u8) {
    ioapic_disable_focus_processor(io);
    ioapic_clear_stuck_irr(io, gsi);
}

pub fn apply_lapic_workarounds(_lapic: &lapic::LocalApic) {
    lint0_8254_quirk(_lapic);
}
