# vahi-apic

x86_64 APIC subsystem — Local APIC, I/O APIC, and MSI vector allocator.

## Modules

| Module | Lines | Purpose |
|--------|-------|---------|
| `lib.rs` | 428 | LAPIC register access, IPI, EOI, IRQ routing |
| `lapic.rs` | 236 | Local APIC init, timer calibration, PIT/CPID |
| `ioapic.rs` | 92 | I/O APIC MMIO driver |
| `msi.rs` | 85 | MSI vector allocator (bitmap-backed) |
| `errata.rs` | 24 | Chipset errata workarounds |

## Dependency Breaking

Original kernel APIC depends on `acpi` and `memory` — creating cycles.
This crate breaks them via traits:

```rust
pub trait AcpiProvider: Send + Sync {
    fn lapic_addr(&self) -> Option<u64>;
    fn overrides(&self) -> &[InterruptOverride];
    fn ioapic_addrs(&self) -> &[u64];
}

pub trait MemoryProvider: Send + Sync {
    fn physical_memory_offset(&self) -> u64;
}
```

The kernel registers implementations before calling `apic::init()`.

## x2APIC Support

Automatically detects x2APIC mode via CPUID and IA32_APIC_BASE MSR.
All register access transparently routes through MSR reads/writes
when x2APIC is active.

## IPI Delivery

```rust
send_ipi(dest_lapic_id, vector, delivery_mode);
send_broadcast_ipi(vector);
send_nmi(dest_lapic_id, vector);
```

## PIC Fallback

Includes pic8259 chained PIC initialization as a fallback for
systems without IOAPIC.
