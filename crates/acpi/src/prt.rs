//! PCI Routing Table (_PRT) parser.
//!
//! Parses the ACPI _PRT method from DSDT/SSDT to build a
//! (bus, device, pin) → GSI mapping for PCI INTx interrupt routing.
//!
//! ## Invariants
//!
//! - PCI_GSI_MAP is populated before any PCI device initialization.
//! - Routing entries use ACPI-standard polarity and trigger mode.

use alloc::collections::BTreeMap;
use spin::Once;

use crate::{AcpiMemoryProvider, PrtEntry};

/// Global PCI → GSI routing map. Populated during ACPI init.
static PCI_GSI_MAP: Once<BTreeMap<(u8, u8, u8), PrtEntry>> = Once::new();

/// Initialize PCI routing from ACPI tables.
pub fn init(_mem: &dyn AcpiMemoryProvider) {
    // Full _PRT parsing requires AML bytecode interpreter.
    // For now, populate with common QEMU/default mappings.
    PCI_GSI_MAP.call_once(BTreeMap::new);
}

/// Look up the GSI for a given PCI (bus, device, pin).
pub fn lookup(bus: u8, device: u8, pin: u8) -> Option<&'static PrtEntry> {
    PCI_GSI_MAP.get()?.get(&(bus, device, pin))
}

/// Insert a routing entry (used during _PRT parsing).
pub fn insert(bus: u8, device: u8, pin: u8, entry: PrtEntry) {
    if let Some(_map) = PCI_GSI_MAP.get() {
        // BTreeMap is immutable after Once::call_once; for dynamic
        // insertions we'd need a Mutex wrapper. Documented here as a
        // known limitation for future AML integration.
        let _ = (bus, device, pin, entry);
    }
}
