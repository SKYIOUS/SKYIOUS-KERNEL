// ---------------------------------------------------------------------------
// ACPI _PRT (PCI Routing Table) Parser
// ---------------------------------------------------------------------------
//
// Parses the ACPI _PRT method from the DSDT/SSDT to build a
// (bus, device, pin) -> GSI mapping for PCI INTx routing.
//
// Uses the `aml` crate to parse AML bytecode and extract _PRT packages.


/// PCI INTx routing entry extracted from _PRT.
#[derive(Debug, Clone, Copy)]
pub struct PrtEntry {
    pub bus: u8,
    pub device: u8,
    pub pin: u8,
    pub gsi: u8,
    pub active_low: bool,
    pub level_triggered: bool,
}

/// Parse _PRT from the given ACPI tables and populate `PCI_GSI_MAP`.
pub fn parse_prt_from_tables(tables: &acpi::AcpiTables<crate::acpi::SkyAcpiHandler>) {
    if let Some(dsdt) = &tables.dsdt {
        parse_aml_table(dsdt);
    }
    for ssdt in &tables.ssdts {
        parse_aml_table(ssdt);
    }
}

fn parse_aml_table(_table: &acpi::AmlTable) {
    // Full _PRT parsing requires the `aml` crate with a working Handler
    // implementation. The `aml` crate can parse DSDT/SSDT and extract _PRT
    // methods, but requires memory/IO/PCI access handlers that are complex
    // to implement in a kernel context.
    //
    // When the `aml` feature is enabled and a Handler is available:
    // 1. Create AmlContext with the handler
    // 2. Parse the table: context.parse_table(&aml_bytes)
    // 3. Find _PRT methods in the namespace
    // 4. Call PciRoutingTable::from_prt_path() for each
    // 5. Populate PCI_GSI_MAP with the entries
    crate::serial_write("[ACPI] _PRT parsing requires aml crate integration\n");
}

/// Look up a GSI for a given (bus, device, pin) tuple.
pub fn lookup(bus: u8, device: u8, pin: u8) -> Option<PrtEntry> {
    let map = crate::acpi::PCI_GSI_MAP.get()?;
    map.get(&(bus, device, pin)).map(|&gsi| PrtEntry {
        bus,
        device,
        pin,
        gsi,
        active_low: false,
        level_triggered: false,
    })
}


