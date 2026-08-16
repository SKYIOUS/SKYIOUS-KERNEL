// ---------------------------------------------------------------------------
// Chipset errata workarounds
// ---------------------------------------------------------------------------
//
// These workarounds mirror critical fixes from Linux (arch/x86/kernel/apic/)
// and the Microsoft HAL. They are gated behind debug_assertions so release
// builds incur zero overhead until a specific errata is confirmed present
// on production hardware.
//
// When an errata is confirmed on a given system, gate it on a CPUID/model
// check and remove the debug_assertions gate.

use crate::apic::ioapic;

/// IOAPIC focus processor bug (Intel 82093AA, early PIIX/PIIX4).
///
/// Some early IOAPICs get "stuck" when high-frequency level-triggered
/// interrupts repeatedly mask/unmask the same redirection entry. Clearing
/// bit 0 (focus processor) in IOAPICVER prevents the stuck condition.
///
/// Linux: `io_apic.c` disables focus processor on affected chipsets.
/// Windows: HAL quirk table.
pub fn ioapic_disable_focus_processor(_io: &mut ioapic::IoApic) {
    #[cfg(debug_assertions)]
    crate::serial_write("[APIC-ERRATA] disable focus processor (debug)\n");
    // TODO: read IOAPICVER, clear bit 0, write back when errata is confirmed
    // on a specific IOAPIC version.
}

/// Stuck IRR workaround.
///
/// If an IOAPIC redirection entry is masked for an extended period and the
/// interrupt never clears, the IRR bit may remain set. Unmask/remask clears
/// the stale state.
///
/// Linux: `mask_ioapic_entries()` + `unmask_ioapic_entry()` during suspend/resume.
pub fn ioapic_clear_stuck_irr(_io: &mut ioapic::IoApic, _gsi: u8) {
    #[cfg(debug_assertions)]
    crate::serial_write("[APIC-ERRATA] clear stuck IRR (debug)\n");
    // TODO: read entry, clear mask bit, write back, set mask bit again.
}

/// 8254 LVT0 timer mode quirk.
///
/// On some chipsets, LVT0 in ExtINT mode conflicts with the 8254 PIC timer.
/// If the LAPIC timer is the only timer source, ensure LVT0 is masked or
/// configured for a non-ExtINT mode.
///
/// Linux: `lapic_init_clockevent()` checks for TSC deadline timer first.
/// Windows: HAL selects timer source based on ACPI_FADT flags.
pub fn lint0_8254_quirk(_lapic: &crate::apic::lapic::LocalApic) {
    #[cfg(debug_assertions)]
    crate::serial_write("[APIC-ERRATA] LVT0/8254 quirk check (debug)\n");
    // TODO: read LVT_LINT0, if delivery mode == ExtINT (0b111) and
    // 8254 is present, log warning or mask entry.
}

/// Run all applicable errata workarounds during IOAPIC initialization.
pub fn apply_ioapic_workarounds(io: &mut ioapic::IoApic, gsi: u8) {
    ioapic_disable_focus_processor(io);
    ioapic_clear_stuck_irr(io, gsi);
}

/// Run all applicable errata workarounds during LAPIC initialization.
pub fn apply_lapic_workarounds(_lapic: &crate::apic::lapic::LocalApic) {
    lint0_8254_quirk(_lapic);
}

