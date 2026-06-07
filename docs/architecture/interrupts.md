# Interrupt Handling Architecture

SkyOS uses the x86 APIC architecture for interrupt management, with support for both legacy PIC and modern MSI-X.

## Interrupt Descriptor Table

The IDT is set up during boot with entries for all 256 interrupt vectors. Each entry specifies a handler function, privilege level, and interrupt stack table (IST) index.

```rust
#[repr(C, packed)]
pub struct IdtEntry {
    handler_low: u16,
    gdt_selector: u16,
    options: IdtOptions,
    handler_mid: u16,
    handler_high: u32,
    reserved: u32,
}
```

## APIC and I/O APIC

The local APIC handles CPU-local interrupts (timers, IPIs), while the I/O APIC distributes hardware IRQs from peripherals. The kernel programs the I/O APIC to route IRQs to specific cores for load distribution.

## IRQ Handling Flow

1. Hardware interrupt fires → I/O APIC delivers to target CPU's local APIC
2. CPU saves registers and enters interrupt handler via IDT
3. Handler acknowledges the interrupt (EOI)
4. The interrupt is converted to an async event and queued for the relevant driver
5. Return from interrupt restores saved registers

## Spurious Interrupts

The kernel handles spurious interrupts by masking the corresponding IRQ line and logging the event. Spurious interrupts typically indicate electrical noise or hardware glitches on the interrupt line.

## Interrupt Safety

All interrupt handlers run on dedicated interrupt stacks (IST1-IST7) to avoid stack overflow. Locks within interrupt handlers use spinlocks with interrupt disabling to prevent deadlocks.
