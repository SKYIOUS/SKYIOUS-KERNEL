# vahi-acpi

ACPI table parsing and PCI interrupt routing.

## Modules

| Module | Contents |
|--------|----------|
| `prt` | PCI Routing Table (_PRT) lookup |
| `madt` | MADT parser (IOAPIC, LAPIC, overrides) |

## ACPI Table Types

| Table | Purpose |
|-------|---------|
| RSDP | Root pointer to all other tables |
| DSDT | Differentiated System Description Table (AML) |
| SSDT | Secondary System Description Table |
| MADT | Multiple APIC Description Table |
| MCFG | PCI Express memory-mapped configuration |
| HPET | High Precision Event Timer |

## Trait Interface

```rust
pub trait AcpiMemoryProvider: Send + Sync {
    fn phys_to_virt(&self, phys: u64) -> u64;
}
```

Breaks the ACPI → memory cycle.

## MADT Parsing

The `madt::parse_madt()` function handles:
- I/O APIC entries (MMIO address, GSI base)
- Interrupt source overrides (ISA → GSI remapping)
- Local APIC NMI configuration
- Processor local APIC discovery
