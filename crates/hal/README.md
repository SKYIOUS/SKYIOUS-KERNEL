# vahi-hal

Hardware Abstraction Layer — platform-independent interfaces for IRQ
controllers, platform info, and timer sources.

## Modules

| Module | Purpose |
|--------|---------|
| `irq` | IRQ controller trait (EOI, mask, route, affinity) |
| `platform` | Platform info (arch, CPU count, frequency, RAM) |
| `timer` | TSC-based timer with trait for other sources |

## Architecture

```text
                    vahi-hal (this crate)
                         │
            ┌────────────┼────────────┐
            │            │            │
     IrqController  PlatformInfo  TimerSource
            │            │            │
     ┌──────┴──────┐    │     ┌──────┴──────┐
     │             │    │     │             │
  LAPIC        IOAPIC  │    TSC        HPET
  (x86)        (x86)  │   (x86)      (x86)
                    GICv2
                   (aarch64)
```

## Migration from Kernel

The kernel's `hal/` module contains three files:
- `irq.rs` → moved to this crate's `irq` module
- `platform.rs` → moved to this crate's `platform` module  
- `timer.rs` → moved to this crate's `timer` module

Each depends only on `IrqSafeMutex` from `vahi-sync`.
