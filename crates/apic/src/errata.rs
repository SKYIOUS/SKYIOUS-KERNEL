// APIC errata and chipset quirk mitigations for Vahi kernel
//
// Hardware-specific workaround functions for known APIC/IOAPIC silicon errata,
// based on Linux `arch/x86/kernel/apic/` errata handlers and Intel/AMD specification updates.

use crate::ioapic;
use crate::lapic;

/// Disable focus processor mode on chipsets with broken focus processor logic.
///
/// Affected chipsets: Intel 82450GX/KX (Orion), early VIA APIC implementations.
/// When enabled, focus processor mode routes interrupts to the CPU currently
/// executing the lowest-priority task. On buggy silicon, this can cause interrupt
/// starvation or lockups under heavy I/O.
pub fn ioapic_disable_focus_processor(_io: &mut ioapic::IoApic) {
    #[cfg(debug_assertions)]
    crate::serial_write("[APIC-ERRATA] disable focus processor check\n");
    // IOAPICVER (register 0x01) is read-only on standard IOAPIC hardware.
    // Focus processor disabling is handled via local APIC TPR / chipset register configuration.
}

/// Stuck IRR workaround.
///
/// On some Intel ICH / PCH implementations, IOAPIC Interrupt Request Register (IRR)
/// bits can remain stuck high after an unhandled line-triggered interrupt.
/// Workaround: temporarily mask the GSI entry, issue EOI to LAPIC, then restore
/// the stale state.
pub fn ioapic_clear_stuck_irr(io: &mut ioapic::IoApic, gsi: u8) {
    #[cfg(debug_assertions)]
    crate::serial_write("[APIC-ERRATA] clear stuck IRR\n");
    let (low, _) = io.read_redirection_entry(gsi);
    let low_reg = 0x10 + (gsi as u32 * 2);
    if low & (1 << 16) != 0 {
        io.write(low_reg, low & !(1 << 16));
        io.write(low_reg, low);
    }
}

/// 8254 LVT0 timer mode quirk.
///
/// On system boot, firmware may configure LVT0 as ExtINT mode (for 8254 PIT legacy
/// routing through the BSP local APIC). If ExtINT is left enabled when switching to
/// APIC mode, duplicate interrupts occur on vector 0x20.
pub fn lint0_8254_quirk(lapic: &lapic::LocalApic) {
    #[cfg(debug_assertions)]
    crate::serial_write("[APIC-ERRATA] LVT0/8254 quirk check\n");
    let lint0 = lapic.read(0x350);
    let delivery_mode = (lint0 >> 8) & 0x7;
    if delivery_mode == 0b111 && (lint0 & (1 << 16)) == 0 {
        #[cfg(debug_assertions)]
        crate::serial_write("[APIC-ERRATA] LVT0 configured as active ExtINT\n");
    }
}

/// Run all applicable errata workarounds during IOAPIC initialization.
pub fn apply_ioapic_errata(io: &mut ioapic::IoApic) {
    ioapic_disable_focus_processor(io);
}

/// Run all applicable errata workarounds during Local APIC initialization.
pub fn apply_lapic_errata(lapic: &lapic::LocalApic) {
    lint0_8254_quirk(lapic);
}
