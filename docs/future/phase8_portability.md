# Phase 8: Portability to Other Architectures

Phase 8 expands SkyOS to run on additional hardware architectures.

## Goals

- aarch64 (ARMv8-A) support
- RISC-V (64-bit) support
- Architecture abstraction layer cleanup
- Cross-platform driver model

## Architecture Support

### aarch64

Target devices: Raspberry Pi 4/5, QEMU virt, Apple Silicon (via virtualization)

Required changes:
- Page table format (4-level vs 3-level page tables)
- Interrupt controller (GICv2/v3 vs APIC)
- Timer (generic timer vs HPET/PIT)
- Boot process (device tree vs ACPI)
- Context switch (register file differences)

### RISC-V

Target devices: QEMU virt, SiFive HiFive Unleashed

Required changes:
- Supervisor mode and SBI interface
- Page table format (Sv39/Sv48)
- PLIC interrupt controller
- Timer via SBI

## Architecture Abstraction

The kernel will define an architecture interface:

```rust
pub trait Arch {
    type PageTable: PageTableOps;
    type InterruptController: InterruptOps;
    type Timer: TimerOps;
    type Context: ContextOps;

    fn init_bsp() -> Result<()>;
    fn init_ap(cpu_id: usize) -> Result<()>;
    fn halt() -> !;
}
```

All architecture-specific code is isolated behind this trait. Platform-independent kernel code uses only the trait interface.

## Expected Timeline

6-12 months per architecture (significant effort).
