//! # vahi-pci - PCI Bus Enumeration & Configuration
//!
//! x86_64 PCI configuration space access via I/O ports 0xCF8/0xCFC,
//! BAR reading, and capability walking.
//!
//! ## Invariants
//!
//! - **Enumeration** runs once on the boot path before drivers init
//! - **BAR mapping** uses the HHDM (Higher-Half Direct Map) from Limine
//! - **MSI** vectors are allocated from the APIC MSI pool

#![no_std]

extern crate alloc;

use x86_64::instructions::port::Port;

/// Read a 32-bit PCI config register.
pub fn read_config_u32(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    let address: u32 = ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC)
        | 0x80000000;

    let mut config_addr = Port::new(0xCF8);
    let mut config_data: Port<u32> = Port::new(0xCFC);

    unsafe {
        config_addr.write(address);
        config_data.read()
    }
}

/// Read a 16-bit PCI config register.
pub fn read_config_u16(bus: u8, slot: u8, func: u8, offset: u8) -> u16 {
    (read_config_u32(bus, slot, func, offset) >> ((offset & 2) * 8)) as u16
}

/// Read an 8-bit PCI config register.
pub fn read_config_u8(bus: u8, slot: u8, func: u8, offset: u8) -> u8 {
    (read_config_u32(bus, slot, func, offset) >> ((offset & 3) * 8)) as u8
}

/// Write a 32-bit PCI config register.
pub fn write_config_u32(bus: u8, slot: u8, func: u8, offset: u8, value: u32) {
    let address: u32 = ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC)
        | 0x80000000;

    let mut config_addr = Port::new(0xCF8);
    let mut config_data: Port<u32> = Port::new(0xCFC);

    unsafe {
        config_addr.write(address);
        config_data.write(value);
    }
}

/// Write a 16-bit PCI config register.
pub fn write_config_u16(bus: u8, slot: u8, func: u8, offset: u8, value: u16) {
    let shift = (offset & 2) * 8;
    let mask = 0xFFFFu32 << shift;
    let aligned = read_config_u32(bus, slot, func, offset);
    write_config_u32(
        bus,
        slot,
        func,
        offset,
        (aligned & !mask) | ((value as u32) << shift),
    );
}

/// Read a 64-bit BAR value.
pub fn read_bar64(bus: u8, slot: u8, func: u8, bar_offset: u8) -> u64 {
    let lo = read_config_u32(bus, slot, func, bar_offset);
    if lo & 0x6 == 0x4 {
        let hi = read_config_u32(bus, slot, func, bar_offset + 4) as u64;
        (hi << 32) | (lo as u64 & 0xFFFFFFF0)
    } else {
        (lo & 0xFFFFFFF0) as u64
    }
}

/// Walk PCI capabilities list, return offset of matching capability ID.
pub fn find_capability(bus: u8, slot: u8, func: u8, cap_id: u8) -> Option<u8> {
    let status = read_config_u16(bus, slot, func, 0x06);
    if status & (1 << 4) == 0 {
        return None;
    }
    let mut offset = read_config_u8(bus, slot, func, 0x34);
    while offset != 0 {
        if read_config_u8(bus, slot, func, offset) == cap_id {
            return Some(offset);
        }
        offset = read_config_u8(bus, slot, func, offset + 1);
    }
    None
}

/// Enable MSI interrupts for a PCI device.
///
/// Finds the MSI capability, programs the APIC vector, and enables
/// MSI delivery. Returns the allocated vector number on success.
///
/// # Implementation Status
///
/// Requires the APIC MSI allocator (vahi-apic::msi) to be wired.
/// Currently returns `None` — callers should fall back to MSI-X or INTx.
pub fn pci_enable_msi(bus: u8, slot: u8, func: u8) -> Option<u8> {
    let cap = find_capability(bus, slot, func, 0x05)?; // PCI_CAP_MSI
    let msg_ctrl = read_config_u16(bus, slot, func, cap + 2);
    let _ = msg_ctrl; // TODO: allocate vector via vahi_apic::msi::alloc()
    None // Not yet wired
}

/// Route a legacy INTx IRQ for a PCI device.
///
/// Maps the device's IRQ pin (A/B/C/D) to an IOAPIC GSI via
/// the ACPI PCI routing table (_PRT).
///
/// # Implementation Status
///
/// Requires ACPI _PRT parsing (vahi-acpi::prt) to be wired.
/// Currently returns `None` — callers should use MSI/MSI-X instead.
pub fn pci_route_legacy_irq(bus: u8, slot: u8, func: u8, irq: u8) -> Option<u8> {
    let _ = (bus, slot, func, irq);
    // TODO: look up via vahi_acpi::prt::lookup(bus, slot, pin)
    None // Not yet wired
}
