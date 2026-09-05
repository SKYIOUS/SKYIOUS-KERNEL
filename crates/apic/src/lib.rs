#![no_std]

//! # vahi-apic
//!
//! x86_64 Advanced Programmable Interrupt Controller (APIC) subsystem:
//! Local APIC, I/O APIC, MSI vector allocator, and IPI dispatch.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────┐    MMIO     ┌──────────────┐
//! │  Local APIC  │◄───────────►│   CPU Core   │
//! └──────┬───────┘             └──────────────┘
//!        │ IPI
//!        ▼
//! ┌──────────────┐    MMIO     ┌──────────────┐
//! │   I/O APIC   │◄───────────►│ PCI / Legacy │
//! └──────────────┘             │   Devices    │
//!                              └──────────────┘
//! ```
//!
//! ## Module Breakdown
//!
//! | Module | Purpose |
//! |--------|---------|
//! | `lapic` | Local APIC init, timer calibration, register access |
//! | `ioapic` | I/O APIC MMIO and redirection table management |
//! | `msi` | MSI vector allocator (bitmap-backed) |
//! | `errata` | Chipset errata workarounds (Intel/AMD quirks) |
//!
//! ## Dependency Breaking
//!
//! This crate cannot depend on `vahi-acpi` or `vahi-memory` (would create
//! cycles). Instead, the kernel provides data via traits during `init()`:
//!
//! - [`AcpiProvider`] — MADT data (LAPIC address, overrides, I/O APIC addresses)
//! - [`MemoryProvider`] — physical memory offset for MMIO mapping
//! - [`SerialWriter`] — debug logging
//!
//! ## xAPIC vs x2APIC
//!
//! The crate transparently supports both modes via [`ApicMode::detect`]:
//!
//! - **xAPIC**: MMIO-mapped registers at `0xFEE00000`
//! - **x2APIC**: MSR-based access (faster, supports > 256 cores)
//!
//! ## Invariants
//!
//! - The APIC subsystem must be initialized exactly once via [`init`]
//! - LAPIC ID 0 is the BSP (bootstrap processor)
//! - EOI must be sent before re-enabling interrupts for the vector
//! - TPR (task priority register) starts at 0 (accept all interrupts)
//! - IPI delivery may fail on bare metal; [`wait_for_ipi`] returns success status
//!
//! ## Initialization Order
//!
//! 1. Kernel parses ACPI MADT, registers [`AcpiProvider`]
//! 2. Kernel registers [`MemoryProvider`] (HHDM offset)
//! 3. Kernel registers [`SerialWriter`]
//! 4. Kernel calls [`init`] — sets up LAPIC, I/O APIC, MSI bitmap
//! 5. After init, drivers call [`route_pci_irq`] / [`route_by_gsi`] to wire interrupts
//!
//! ## Safety
//!
//! MMIO reads/writes use `read_volatile`/`write_volatile`. x2APIC MSR access
//! uses `rdmsr`/`wrmsr` with `preserves_flags`. All unsafe blocks document
//! their invariants.

extern crate alloc;

pub mod errata;
pub mod ioapic;
pub mod lapic;
pub mod msi;

// ─── ACPI Data Provider Trait ────────────────────────────────────────
// The kernel implements this with real ACPI data. APIC reads it during init.

/// Interrupt override entry from ACPI MADT.
pub struct InterruptOverride {
    pub isa_irq: u8,
    pub polarity: Polarity,
    pub trigger_mode: TriggerMode,
    pub global_system_interrupt: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    ActiveHigh,
    ActiveLow,
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerMode {
    Edge,
    Level,
    Default,
}

/// Trait for providing ACPI data to the APIC subsystem.
/// Implemented by the kernel's acpi module.
pub trait AcpiProvider: Send + Sync {
    fn lapic_addr(&self) -> Option<u64>;
    fn overrides(&self) -> &[InterruptOverride];
    fn ioapic_addrs(&self) -> &[u64];
}

/// Trait for providing physical memory offset to the APIC subsystem.
/// Implemented by the kernel's memory module.
pub trait MemoryProvider: Send + Sync {
    fn physical_memory_offset(&self) -> u64;
}

/// Trait for serial output (debug logging).
pub trait SerialWriter: Send + Sync {
    fn write(&self, msg: &str);
}

// ─── PIC constants ──────────────────────────────────────────────

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: vahi_sync::IrqSafeMutex<pic8259::ChainedPics> =
    vahi_sync::IrqSafeMutex::new(unsafe { pic8259::ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// Monotonic 100 Hz tick count (delegates to the kernel clock registered
/// in `vahi-types` — the single shared clock, no per-crate extern stubs).
pub fn get_ticks() -> u64 {
    vahi_types::get_ticks()
}

// ─── Interrupt vector indices ───────────────────────────────────

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = 32,
    Keyboard = 33,
    _PageFault = 14,
    Mouse = 44,
    Network = 43,
    TlbFlush = 250,
    IpiFunc = 251,
}

impl InterruptIndex {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}

// ─── x2APIC support ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApicMode {
    Xapic,
    X2Apic,
}

impl ApicMode {
    pub fn detect() -> Self {
        let has_x2apic = unsafe {
            let mut ecx: u32;
            core::arch::asm!(
                "push rbx",
                "mov eax, 0x1",
                "cpuid",
                "pop rbx",
                lateout("ecx") ecx,
                lateout("edx") _,
                lateout("eax") _,
                options(nostack, preserves_flags)
            );
            (ecx & (1 << 21)) != 0
        };

        if !has_x2apic {
            return ApicMode::Xapic;
        }

        let x2apic_enabled = unsafe {
            (x86_64::registers::model_specific::Msr::new(IA32_APIC_BASE_MSR).read() >> 10) & 1 != 0
        };

        if x2apic_enabled {
            ApicMode::X2Apic
        } else {
            ApicMode::Xapic
        }
    }
}

static MODE: spin::Once<ApicMode> = spin::Once::new();

pub fn mode() -> ApicMode {
    *MODE.get().unwrap_or(&ApicMode::Xapic)
}

pub fn is_x2apic() -> bool {
    mode() == ApicMode::X2Apic
}

const IA32_APIC_BASE_MSR: u32 = 0x1B;

// ─── LAPIC register offsets ─────────────────────────────────────

pub const LAPIC_PHYS_BASE: u64 = 0xfee00000;
pub const LAPIC_ID: u32 = 0x20;
pub const LAPIC_VERSION: u32 = 0x30;
pub const LAPIC_TPR: u32 = 0x80;
pub const LAPIC_EOI: u32 = 0xB0;
pub const LAPIC_SPURIOUS: u32 = 0xF0;
pub const LAPIC_LVT_TIMER: u32 = 0x320;
pub const LAPIC_LVT_LINT0: u32 = 0x350;
pub const LAPIC_LVT_LINT1: u32 = 0x360;
pub const LAPIC_LVT_ERROR: u32 = 0x370;
pub const LAPIC_TIMER_ICR: u32 = 0x380;
pub const LAPIC_TIMER_CCR: u32 = 0x390;
pub const LAPIC_TIMER_DCR: u32 = 0x3E0;

// ─── ICR bits ───────────────────────────────────────────────────

pub const ICR_LOW: u32 = 0x300;
pub const ICR_HIGH: u32 = 0x310;
pub const ICR_DELIVERY_PENDING: u32 = 1 << 12;
pub const ICR_ASSERT: u32 = 1 << 14;
pub const ICR_SHORTHAND_ALL_EXCL_SELF: u32 = 0x3 << 18;
pub const ICR_DELIVERY_MODE_SHIFT: u32 = 8;
pub const ICR_DELIVERY_MODE_FIXED: u8 = 0;
pub const ICR_DELIVERY_MODE_SMI: u8 = 2;
pub const ICR_DELIVERY_MODE_NMI: u8 = 4;
pub const ICR_DELIVERY_MODE_INIT: u8 = 5;
pub const ICR_DELIVERY_MODE_SIPI: u8 = 6;
pub const ICR_DELIVERY_MODE_PMI: u8 = 15;
pub const ICR_DEST_SHORTHAND_NONE: u32 = 0;

// ─── Global state (set during init) ─────────────────────────────

static ACPI: spin::Once<&'static dyn AcpiProvider> = spin::Once::new();
static MEM: spin::Once<&'static dyn MemoryProvider> = spin::Once::new();
static SERIAL: spin::Once<&'static dyn SerialWriter> = spin::Once::new();

/// Internal helper: get physical memory offset.
fn pmo() -> u64 {
    MEM.get()
        .expect("vahi-apic: MemoryProvider not initialized")
        .physical_memory_offset()
}

/// Internal helper: log a message.
fn log(msg: &str) {
    if let Some(w) = SERIAL.get() {
        w.write(msg);
    }
}

// ─── Priority constants ─────────────────────────────────────────

pub mod priority {
    pub const EXCEPTION: u8 = 0x00;
    pub const LEGACY: u8 = 0x10;
    pub const DEVICE: u8 = 0x20;
    pub const TIMER: u8 = 0x30;
    pub const IPI: u8 = 0x40;
    pub const SPURIOUS: u8 = 0xF0;
}

// ─── TPR ────────────────────────────────────────────────────────

pub fn set_tpr(priority: u8) {
    lapic_write32(LAPIC_TPR, (priority & 0xF0) as u32);
}

pub fn tpr() -> u8 {
    (lapic_read32(LAPIC_TPR) & 0xFF) as u8
}

// ─── Initialization ─────────────────────────────────────────────

/// Initialize the APIC subsystem.
///
/// # Safety
///
/// Must be called after:
/// - `AcpiProvider` is populated (MADT parsed)
/// - `MemoryProvider` returns a valid physical memory offset
/// - `SerialWriter` is ready for output
pub fn init(
    acpi: &'static dyn AcpiProvider,
    mem: &'static dyn MemoryProvider,
    serial: &'static dyn SerialWriter,
) {
    ACPI.call_once(|| acpi);
    MEM.call_once(|| mem);
    SERIAL.call_once(|| serial);

    MODE.call_once(ApicMode::detect);
    set_tpr(0);
    lapic::init();
    msi::init();

    let lapic_id = current_lapic_id();

    for &addr in acpi.ioapic_addrs() {
        let mut ioapic = unsafe { ioapic::IoApic::new(addr as usize) };

        let (kbd_active_low, kbd_level, _kbd_gsi) = override_flags(1);
        ioapic.set_redirection(1, 33, lapic_id, kbd_active_low, kbd_level, false);
        let (mse_active_low, mse_level, _mse_gsi) = override_flags(12);
        ioapic.set_redirection(12, 44, lapic_id, mse_active_low, mse_level, false);

        log("[I/O APIC] Initialized\n");
    }
}

// ─── ACPI Override Resolution ───────────────────────────────────

fn override_flags(isa_irq: u8) -> (bool, bool, u8) {
    if let Some(acpi) = ACPI.get() {
        if let Some(o) = acpi.overrides().iter().find(|o| o.isa_irq == isa_irq) {
            let active_low = o.polarity == Polarity::ActiveLow;
            let level = o.trigger_mode == TriggerMode::Level;
            let gsi = o.global_system_interrupt.min(255) as u8;
            return (active_low, level, gsi);
        }
    }
    (false, false, isa_irq)
}

fn override_flags_by_gsi(gsi: u8) -> (bool, bool) {
    if let Some(acpi) = ACPI.get() {
        if let Some(o) = acpi
            .overrides()
            .iter()
            .find(|o| o.global_system_interrupt == gsi as u32)
        {
            let active_low = o.polarity == Polarity::ActiveLow;
            let level = o.trigger_mode == TriggerMode::Level;
            return (active_low, level);
        }
    }
    (false, false)
}

// ─── LAPIC Register Access ──────────────────────────────────────

pub fn current_lapic_id() -> u8 {
    (lapic_read32(LAPIC_ID) >> 24) as u8
}

pub fn lapic_read32(offset: u32) -> u32 {
    if is_x2apic() {
        x2apic_read(offset)
    } else {
        let ptr = (pmo() + LAPIC_PHYS_BASE + offset as u64) as *const u32;
        // SAFETY: LAPIC register space is MMIO-mapped at the physical
        // address from MADT. `pmo()` returns the HHDM offset.
        unsafe { core::ptr::read_volatile(ptr) }
    }
}

fn x2apic_read(offset: u32) -> u32 {
    let msr = 0x800u32 + (offset >> 4);
    let mut val: u64;
    // SAFETY: x2APIC MSRs (0x800-0x8FF) are safe to read from any
    // privilege level when x2APIC is enabled. The offset maps directly
    // to the APIC register space.
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("rdx") _,
            out("rax") val,
            options(nostack, preserves_flags)
        );
    }
    val as u32
}

fn lapic_write32(offset: u32, value: u32) {
    if is_x2apic() {
        x2apic_write(offset, value)
    } else {
        let ptr = (pmo() + LAPIC_PHYS_BASE + offset as u64) as *mut u32;
        // SAFETY: LAPIC MMIO register space. Volatile write ensures the
        // compiler does not reorder or elide the store to hardware.
        unsafe {
            core::ptr::write_volatile(ptr, value);
        }
    }
}

fn x2apic_write(offset: u32, value: u32) {
    let msr = 0x800u32 + (offset >> 4);
    let val = value as u64;
    // SAFETY: x2APIC MSR writes control interrupt delivery, EOI,
    // ICR (inter-processor interrupt), etc. Caller must ensure the
    // write is semantically correct for the target register.
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("rdx") 0u32,
            in("rax") val,
            options(nostack, preserves_flags)
        );
    }
}

// ─── IPI ────────────────────────────────────────────────────────

pub fn send_ipi(dest_lapic_id: u8, vector: u8, delivery_mode: u8) {
    lapic_write32(ICR_HIGH, (dest_lapic_id as u32) << 24);
    lapic_write32(
        ICR_LOW,
        ICR_ASSERT | ((delivery_mode as u32) << ICR_DELIVERY_MODE_SHIFT) | vector as u32,
    );
}

pub fn send_broadcast_ipi(vector: u8) {
    lapic_write32(ICR_HIGH, 0);
    lapic_write32(
        ICR_LOW,
        ICR_SHORTHAND_ALL_EXCL_SELF | ICR_ASSERT | vector as u32,
    );
}

pub fn send_nmi(dest_lapic_id: u8, vector: u8) {
    send_ipi(dest_lapic_id, vector, ICR_DELIVERY_MODE_NMI);
}

pub fn send_smi(dest_lapic_id: u8) {
    send_ipi(dest_lapic_id, 0, ICR_DELIVERY_MODE_SMI);
}

pub fn send_pmi(dest_lapic_id: u8, vector: u8) {
    let mode = if is_x2apic() {
        ICR_DELIVERY_MODE_PMI
    } else {
        ICR_DELIVERY_MODE_FIXED
    };
    send_ipi(dest_lapic_id, vector, mode);
}

pub fn send_lowest_priority(dest_lapic_id: u8, vector: u8) {
    send_ipi(dest_lapic_id, vector, 1);
}

const MAX_IPI_WAIT: u64 = 1_000_000;

pub fn wait_for_ipi() -> bool {
    let mut waited = 0;
    while (lapic_read32(ICR_LOW) & ICR_DELIVERY_PENDING) != 0 {
        core::hint::spin_loop();
        waited += 1;
        if waited >= MAX_IPI_WAIT {
            return false;
        }
    }
    true
}

pub fn eoi() {
    lapic_write32(LAPIC_EOI, 0);
}

// ─── IRQ Routing ────────────────────────────────────────────────

pub fn route_pci_irq(irq: u8, vector: u8) {
    let (active_low, level, gsi) = override_flags(irq);
    route_gsi(gsi, vector, active_low, level);
}

pub fn route_by_gsi(gsi: u8, vector: u8) {
    let (active_low, level) = override_flags_by_gsi(gsi);
    route_gsi(gsi, vector, active_low, level);
}

fn route_gsi(gsi: u8, vector: u8, active_low: bool, level: bool) {
    let id = current_lapic_id();
    if let Some(acpi) = ACPI.get() {
        for &addr in acpi.ioapic_addrs() {
            let mut io = unsafe { ioapic::IoApic::new(addr as usize) };
            if gsi <= io.max_redirection_entry() {
                io.set_redirection(gsi, vector, id, active_low, level, false);
                return;
            }
        }
    }
}
