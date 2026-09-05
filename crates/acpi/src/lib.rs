//! ACPI table parsing and interrupt routing for the Vahi kernel.
//!
//! Provides PCI routing table (_PRT) parsing, MADT interpretation,
//! and platform enumeration. Breaks cycles via trait-based data providers.
//!
//! ## Dependency Breaking
//!
//! This crate cannot depend on `vahi-apic` or `vahi-memory` (would create
//! cycles). Instead, the kernel provides physical address translation via
//! the `MemoryProvider` trait.
//!
//! ## ACPI Table Types
//!
//! | Table | Purpose |
//! |-------|---------|
//! | RSDP  | Root pointer to all other tables |
//! | DSDT  | Differentiated System Description Table (AML bytecode) |
//! | SSDT  | Secondary System Description Table (hotplug, overrides) |
//! | MADT  | Multiple APIC Description Table (IOAPIC, LAPIC, overrides) |
//! | MCFG  | PCI express memory-mapped configuration |
//! | HPET  | High Precision Event Timer |
//!
//! ## Invariants
//!
//! - ACPI table pointers are valid for the lifetime of the kernel.
//! - PCI routing table is populated before any IRQ routing occurs.
//! - MADT overrides are resolved before IOAPIC initialization.

#![no_std]

extern crate alloc;

pub mod madt;
pub mod prt;

/// Trait for providing physical-to-virtual address translation.
/// Implemented by the kernel's memory module.
pub trait AcpiMemoryProvider: Send + Sync {
    /// Translate a physical address to a virtual address via HHDM.
    fn phys_to_virt(&self, phys: u64) -> u64;
}

/// PCI interrupt routing entry from ACPI _PRT.
#[derive(Debug, Clone, Copy)]
pub struct PrtEntry {
    pub bus: u8,
    pub device: u8,
    pub pin: u8,
    pub gsi: u32,
    pub active_low: bool,
    pub level_triggered: bool,
}
