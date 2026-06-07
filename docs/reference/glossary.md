# Kernel Terminology Glossary

This glossary defines common terms used in SkyOS development.

## A

**ACPI** - Advanced Configuration and Power Interface. Standard for hardware discovery and power management.

**APIC** - Advanced Programmable Interrupt Controller. Modern x86 interrupt controller (local APIC + I/O APIC).

**ASLR** - Address Space Layout Randomization. Security technique that randomizes memory layout to prevent exploitation.

## B

**BSP** - Bootstrap Processor. The primary CPU that initializes the system during boot.

**BAR** - Base Address Register. PCI configuration register describing a device's memory or I/O region.

## C

**Capability** - An unforgeable token granting access to a specific resource or operation.

**COW** - Copy-on-Write. Memory sharing technique where pages are shared until one process writes to them.

## D

**DMA** - Direct Memory Access. Hardware feature allowing devices to access memory without CPU involvement.

**DSDT** - Differentiated System Description Table. ACPI table containing device definitions in AML.

## E

**ELC** - Event Loop Core. The heart of the async executor that polls tasks.

**EOI** - End of Interrupt. Signal sent to the APIC indicating interrupt handling is complete.

## F

**Futex** - Fast Userspace Mutex. Synchronization primitive where the fast path avoids kernel involvement.

## G

**GDT** - Global Descriptor Table. x86 data structure defining memory segments (minimal in long mode).

**GOP** - Graphics Output Protocol. UEFI protocol providing framebuffer access.

## I

**IDT** - Interrupt Descriptor Table. x86 data structure mapping interrupt vectors to handlers.

**IPC** - Inter-Process Communication. Mechanisms for data exchange between processes.

**IRQ** - Interrupt Request. A hardware signal requesting CPU attention.

**IST** - Interrupt Stack Table. x86 feature allowing different stack pointers for different interrupt handlers.

## K

**KASAN** - Kernel Address Sanitizer. Runtime tool for detecting memory errors in kernel code.

**KPTI** - Kernel Page Table Isolation. Meltdown mitigation technique separating user/kernel page tables.

## L

**LAPIC** - Local APIC. Per-CPU interrupt controller handling local interrupts and IPIs.

**LTO** - Link-Time Optimization. Whole-program optimization across compilation units.

## M

**MADT** - Multiple APIC Description Table. ACPI table listing APIC information.

**MMIO** - Memory-Mapped I/O. Hardware registers accessed through regular memory load/store instructions.

**MSI** - Message Signaled Interrupt. PCI interrupt mechanism using memory writes instead of dedicated lines.

## N

**NMI** - Non-Maskable Interrupt. Interrupt that cannot be disabled, used for hardware failures.

## P

**PIC** - Programmable Interrupt Controller. Legacy interrupt controller (i8259A), replaced by APIC.

**PIT** - Programmable Interval Timer. Legacy system timer (i8254).

**PML4** - Page Map Level 4. The top-level page table in x86_64 4-level paging.

## S

**SIPI** - Startup IPI. Inter-processor interrupt used to wake application processors during SMP boot.

**SMAP/SMEP** - Supervisor Mode Access/Execution Prevention. CPU features preventing kernel from accessing/executing userspace memory.

## T

**TLS** - Thread-Local Storage. Per-thread data accessed via the FS segment base.

**TSC** - Time Stamp Counter. CPU cycle counter used for high-resolution timing.

## U

**UEFI** - Unified Extensible Firmware Interface. Modern firmware interface replacing legacy BIOS.

## V

**VFS** - Virtual File System. Kernel layer providing a uniform interface to different filesystem implementations.

**vDSO** - Virtual Dynamic Shared Object. Kernel-mapped userspace library for fast syscalls.
