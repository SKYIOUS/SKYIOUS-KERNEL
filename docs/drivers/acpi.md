# ACPI Tables and Configuration

The Advanced Configuration and Power Interface (ACPI) subsystem provides hardware configuration information and power management.

## Table Parsing

The kernel locates the ACPI Root System Description Pointer (RSDP) during boot and parses the table hierarchy:

1. **RSDP**: Root pointer, found in UEFI configuration tables or EBDA
2. **RSDT/XSDT**: Root System Description Table, lists all ACPI tables
3. **FADT**: Fixed ACPI Description Table (power management, wake, reset)
4. **MADT**: Multiple APIC Description Table (CPU/APIC information)
5. **SSDT**: Secondary System Description Table (device-specific data)
6. **DSDT**: Differentiated System Description Table (device definitions)
7. **HPET**: HPET timer information
8. **MCFG**: PCI Express MMIO configuration space base address

```rust
pub struct AcpiTables {
    pub rsdp: Rsdp,
    pub xsdt: Xsdt,
    pub fadt: Option<Fadt>,
    pub madt: Option<Madt>,
    pub hpet: Option<HpetTable>,
    pub mcfg: Option<Mcfg>,
}
```

## MADT Parsing

The MADT contains multiple entries describing interrupt controllers:
- **Local APIC**: Each CPU's local APIC
- **I/O APIC**: I/O APIC with interrupt redirection entries
- **ISO** (Interrupt Source Override): Remapping of legacy IRQs
- **NMI**: NMI source information

## ACPI AML

ACPI defines device behavior using ACPI Machine Language (AML). The kernel includes a minimal AML interpreter for basic operations:
- Power state transitions (S1, S3, S4, S5)
- Device power management
- System reset and shutdown

## Power Management

The ACPI subsystem enables:
```rust
pub fn acpi_shutdown() -> Result<(), DriverError>;
pub fn acpi_reboot() -> Result<(), DriverError>;
pub fn acpi_sleep(state: SleepState) -> Result<(), DriverError>;
```

These functions write to the PM1a control register (from FADT) with the appropriate sleep type values.
