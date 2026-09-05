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

pub fn ioapic_disable_focus_processor(_io: &mut ioapic::IoApic) {
    // TODO: read IOAPICVER, clear bit 0, write back when errata confirmed
}

pub fn ioapic_clear_stuck_irr(_io: &mut ioapic::IoApic, _gsi: u8) {
    // TODO: read entry, clear mask bit, write back, set mask bit again
}

pub fn lint0_8254_quirk(_lapic: &lapic::LocalApic) {
    // TODO: read LVT_LINT0, if ExtINT and 8254 present, log warning
}

pub fn apply_ioapic_workarounds(io: &mut ioapic::IoApic, gsi: u8) {
    ioapic_disable_focus_processor(io);
    ioapic_clear_stuck_irr(io, gsi);
}

pub fn apply_lapic_workarounds(_lapic: &lapic::LocalApic) {
    lint0_8254_quirk(_lapic);
}
