//! MADT (Multiple APIC Description Table) parser.
//!
//! Interprets the MADT to discover:
//! - Local APIC addresses and IDs
//! - I/O APIC MMIO addresses
//! - Interrupt source overrides (ISA → GSI remapping)
//! - Processor topology (LAPIC, x2APIC, local NMI)

use crate::AcpiMemoryProvider;

/// Parsed I/O APIC entry from MADT.
#[derive(Debug, Clone, Copy)]
pub struct IoApicEntry {
    pub id: u8,
    pub mmio_addr: u64,
    pub gsi_base: u32,
}

/// Parsed interrupt override entry from MADT.
#[derive(Debug, Clone, Copy)]
pub struct InterruptOverride {
    pub bus: u8,
    pub source: u8,
    pub gsi: u32,
    pub active_low: bool,
    pub level_triggered: bool,
}

/// Parsed processor local NMI entry.
#[derive(Debug, Clone, Copy)]
pub struct LocalNmi {
    pub processor_id: u8,
    pub lint: u8,
    pub active_low: bool,
    pub level_triggered: bool,
}

/// Result of parsing the MADT.
#[derive(Debug, Default)]
pub struct MadtInfo {
    pub lapic_phys_addr: u64,
    pub ioapics: alloc::vec::Vec<IoApicEntry>,
    pub overrides: alloc::vec::Vec<InterruptOverride>,
    pub local_nmis: alloc::vec::Vec<LocalNmi>,
}

/// Parse the MADT from raw bytes.
///
/// # Safety
///
/// `table_data` must be a valid ACPI MADT table (at least 44 bytes).
/// The table must be aligned to at least a 4-byte boundary for the
/// field reads to be correct.
pub unsafe fn parse_madt(table_data: &[u8], mem: &dyn AcpiMemoryProvider) -> MadtInfo {
    let mut info = MadtInfo::default();

    if table_data.len() < 44 {
        return info;
    }

    // MADT header: 4-byte length at offset 4
    let table_len =
        u32::from_le_bytes([table_data[4], table_data[5], table_data[6], table_data[7]]) as usize;

    // Local APIC address at offset 36
    info.lapic_phys_addr = u32::from_le_bytes([
        table_data[36],
        table_data[37],
        table_data[38],
        table_data[39],
    ]) as u64;

    // Parse APIC structures starting at offset 44
    let mut offset = 44;
    while offset + 2 < table_data.len().min(offset + (table_len - 44)) {
        let entry_type = table_data[offset];
        let entry_len = table_data[offset + 1] as usize;

        if entry_len < 2 || offset + entry_len > table_data.len() {
            break;
        }

        match entry_type {
            // Local APIC
            0 if entry_len >= 8 => {
                // Processor ID, LAPIC ID, flags
            }
            // I/O APIC
            1 if entry_len >= 12 => {
                let id = table_data[offset + 2];
                let mmio_addr = u32::from_le_bytes([
                    table_data[offset + 4],
                    table_data[offset + 5],
                    table_data[offset + 6],
                    table_data[offset + 7],
                ]) as u64;
                let gsi_base = u32::from_le_bytes([
                    table_data[offset + 8],
                    table_data[offset + 9],
                    table_data[offset + 10],
                    table_data[offset + 11],
                ]);
                info.ioapics.push(IoApicEntry {
                    id,
                    mmio_addr,
                    gsi_base,
                });
            }
            // Interrupt Source Override
            2 if entry_len >= 10 => {
                info.overrides.push(InterruptOverride {
                    bus: table_data[offset + 3],
                    source: table_data[offset + 4],
                    gsi: u32::from_le_bytes([
                        table_data[offset + 5],
                        table_data[offset + 6],
                        table_data[offset + 7],
                        table_data[offset + 8],
                    ]),
                    active_low: (table_data[offset + 9] & 0b11) == 0b01,
                    level_triggered: (table_data[offset + 9] >> 2) & 0b11 == 0b01,
                });
            }
            // Local APIC NMI
            4 if entry_len >= 6 => {
                info.local_nmis.push(LocalNmi {
                    processor_id: table_data[offset + 2],
                    lint: table_data[offset + 4],
                    active_low: (table_data[offset + 5] & 0b10) != 0,
                    level_triggered: (table_data[offset + 5] & 0b01) == 0,
                });
            }
            _ => {}
        }

        offset += entry_len;
    }

    // Convert physical LAPIC address via HHDM
    info.lapic_phys_addr = mem.phys_to_virt(info.lapic_phys_addr);

    info
}
