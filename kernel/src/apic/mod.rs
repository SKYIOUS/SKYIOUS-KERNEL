pub mod lapic;
pub mod ioapic;
pub mod msi;
pub mod errata;

// ---------------------------------------------------------------------------
// x2APIC support
// ---------------------------------------------------------------------------
/// Which APIC access mode the kernel is running under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApicMode {
    /// Legacy xAPIC using memory-mapped I/O at `LAPIC_PHYS_BASE`.
    Xapic,
    /// Extended x2APIC using model-specific registers (MSRs 0x802..0x83F).
    X2Apic,
}

impl ApicMode {
    /// Detect the current APIC mode by reading CPUID and IA32_APIC_BASE MSR.
    ///
    /// Returns `X2Apic` only when the CPU supports x2APIC (CPUID.0x1:ECX[21])
    /// and the MSR is already enabled. Otherwise falls back to `Xapic`.
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

        let x2apic_enabled = unsafe { (x86_64::registers::model_specific::Msr::new(IA32_APIC_BASE_MSR).read() >> 10) & 1 != 0 };

        if x2apic_enabled {
            ApicMode::X2Apic
        } else {
            ApicMode::Xapic
        }
    }
}

/// Current APIC access mode, detected once during `apic::init()`.
static MODE: spin::Once<ApicMode> = spin::Once::new();

/// Return the APIC mode detected during initialization.
pub fn mode() -> ApicMode {
    *MODE.get().unwrap_or(&ApicMode::Xapic)
}

/// Return true if the system is running in x2APIC mode.
pub fn is_x2apic() -> bool {
    mode() == ApicMode::X2Apic
}

/// MSR address of IA32_APIC_BASE (0x1B). Bits 10 = x2APIC enable, bit 11 = xAPIC global enable.
const IA32_APIC_BASE_MSR: u32 = 0x1B;
/// Base MSR for x2APIC register window (0x802..0x83F). Offset within the window matches xAPIC offsets.
const X2APIC_BASE_MSR: u32 = 0x802;

// ---------------------------------------------------------------------------
// LAPIC register offsets (Intel SDM Vol. 3, "Local Vector Table" / Ch. 10).
// These are offsets from the LAPIC physical base (see `LAPIC_PHYS_BASE`).
// ---------------------------------------------------------------------------
/// Fixed physical base address of the local APIC MMIO range.
/// Each CPU's access to this address targets its *own* local APIC.
pub const LAPIC_PHYS_BASE: u64 = 0xfee00000;

/// Local APIC ID Register (read: bits 31:24 = initial APIC ID).
pub const LAPIC_ID: u32 = 0x20;
/// Local APIC Version Register (read-only; lower byte = version).
pub const LAPIC_VERSION: u32 = 0x30;
/// Task Priority Register.
pub const LAPIC_TPR: u32 = 0x80;
/// End-of-Interrupt Register (write-only; writes 0 to lower 8 bits).
pub const LAPIC_EOI: u32 = 0xB0;
/// Spurious Interrupt Vector Register.
pub const LAPIC_SPURIOUS: u32 = 0xF0;
/// LVT Timer Register.
pub const LAPIC_LVT_TIMER: u32 = 0x320;
/// LVT LINT0 Register.
pub const LAPIC_LVT_LINT0: u32 = 0x350;
/// LVT LINT1 Register.
pub const LAPIC_LVT_LINT1: u32 = 0x360;
/// LVT Error Register.
pub const LAPIC_LVT_ERROR: u32 = 0x370;
/// Initial Count Register (for the timer).
pub const LAPIC_TIMER_ICR: u32 = 0x380;
/// Current-count register (read-only; decrements at the bus rate while armed).
pub const LAPIC_TIMER_CCR: u32 = 0x390;
/// Divide Configuration Register (timer clock divider).
pub const LAPIC_TIMER_DCR: u32 = 0x3E0;

// ---------------------------------------------------------------------------
// Interrupt Command Register (ICR) bits (Intel SDM Vol. 3 §10.3 / §10.6).
// ---------------------------------------------------------------------------
/// ICR low double-word (offset 0x300).
pub const ICR_LOW: u32 = 0x300;
/// ICR high double-word (offset 0x310): destination LAPIC ID in bits 31:24.
pub const ICR_HIGH: u32 = 0x310;
/// ICR low: bit 12 = Delivery Status (read-only; 1 = pending).
pub const ICR_DELIVERY_PENDING: u32 = 1 << 12;
/// ICR low: bit 14 = Assert (level-triggered IPIs).
pub const ICR_ASSERT: u32 = 1 << 14;
/// ICR low: bits 18-19 = Destination Shorthand; `0b11` = "all excluding self".
pub const ICR_SHORTHAND_ALL_EXCL_SELF: u32 = 0x3 << 18;
/// ICR low: bits 10:8 = Delivery Mode; shift value applied to the mode field.
pub const ICR_DELIVERY_MODE_SHIFT: u32 = 8;
/// Delivery mode 0: Fixed (default).
pub const ICR_DELIVERY_MODE_FIXED: u8 = 0;
/// Delivery mode 2: System Management Interrupt (SMI).
pub const ICR_DELIVERY_MODE_SMI: u8 = 2;
/// Delivery mode 4: Non-Maskable Interrupt (NMI).
pub const ICR_DELIVERY_MODE_NMI: u8 = 4;
/// Delivery mode 5: INIT request.
pub const ICR_DELIVERY_MODE_INIT: u8 = 5;
/// Delivery mode 6: Startup IPI (SIPI).
pub const ICR_DELIVERY_MODE_SIPI: u8 = 6;
/// Delivery mode 15: PMI (x2APIC only).
pub const ICR_DELIVERY_MODE_PMI: u8 = 15;
/// Destination shorthand 0: destination field specifies target APIC ID.
pub const ICR_DEST_SHORTHAND_NONE: u32 = 0;

/// Resolve an interrupt override (if any) for the given ISA IRQ line.
///
/// Returns `(active_low, level_triggered, gsi)` flags suitable for
/// `IoApic::set_redirection`. When no override exists (or `OVERRIDES` was
/// not populated), the ISA bus defaults are returned: active-high, edge,
/// and the GSI defaults to the ISA IRQ itself (standard APIC identity
/// mapping for IRQ 0-15).
fn override_flags(isa_irq: u8) -> (bool, bool, u8) {
    if let Some(overrides) = crate::acpi::OVERRIDES.get() {
        if let Some(o) = overrides.iter().find(|o| o.isa_irq == isa_irq) {
            let active_low = o.polarity == crate::acpi::Polarity::ActiveLow;
            let level = o.trigger_mode == crate::acpi::TriggerMode::Level;
            let gsi = o.global_system_interrupt.min(255) as u8;
            return (active_low, level, gsi);
        }
    }
    (false, false, isa_irq)
}

/// Resolve polarity/trigger flags for a Global System Interrupt (GSI).
///
/// Looks up the override whose `global_system_interrupt` matches `gsi`.
/// Returns ISA bus defaults when no override covers this GSI.
fn override_flags_by_gsi(gsi: u8) -> (bool, bool) {
    if let Some(overrides) = crate::acpi::OVERRIDES.get() {
        if let Some(o) = overrides.iter().find(|o| o.global_system_interrupt == gsi as u32) {
            let active_low = o.polarity == crate::acpi::Polarity::ActiveLow;
            let level = o.trigger_mode == crate::acpi::TriggerMode::Level;
            return (active_low, level);
        }
    }
    (false, false)
}

// ---------------------------------------------------------------------------
// TPR / Vector Priority
// ---------------------------------------------------------------------------
/// Priority classes for APIC vectors. The Task Priority Register (TPR) uses
/// the upper 4 bits of the vector number as the priority class; interrupts
/// with a priority class lower than TPR are masked.
///
/// Vahi does not implement a full IRQL model (unlike Windows), but these
/// constants let the kernel group vectors by importance.
pub mod priority {
    pub const EXCEPTION: u8 = 0x00;
    pub const LEGACY: u8 = 0x10;
    pub const DEVICE: u8 = 0x20;
    pub const TIMER: u8 = 0x30;
    pub const IPI: u8 = 0x40;
    pub const SPURIOUS: u8 = 0xF0;
}

/// Set the Task Priority Register (TPR) of the current CPU.
///
/// Only the upper 4 bits of `priority` are used by the APIC hardware.
/// Pass one of the `priority::*` constants, or 0 to accept all interrupts.
pub fn set_tpr(priority: u8) {
    lapic_write32(LAPIC_TPR, (priority & 0xF0) as u32);
}

/// Read the current Task Priority Register (TPR).
pub fn tpr() -> u8 {
    (lapic_read32(LAPIC_TPR) & 0xFF) as u8
}

pub fn init() {
    MODE.call_once(ApicMode::detect);
    set_tpr(0);
    lapic::init();
    msi::init();

    let lapic_id = current_lapic_id();

    if let Some(ioapic_addrs) = crate::acpi::IOAPIC_ADDRS.get() {
        for &addr in ioapic_addrs {
            let mut ioapic = unsafe { ioapic::IoApic::new(addr) };

            // Keyboard (ISA IRQ 1) and mouse (ISA IRQ 12) use the ISA bus
            // defaults unless an ACPI interrupt override says otherwise.
            let (kbd_active_low, kbd_level, _kbd_gsi) = override_flags(1);
            ioapic.set_redirection(1, 33, lapic_id, kbd_active_low, kbd_level, false);
            let (mse_active_low, mse_level, _mse_gsi) = override_flags(12);
            ioapic.set_redirection(12, 44, lapic_id, mse_active_low, mse_level, false);

            crate::println!("I/O APIC: Initialized at 0x{:x}", addr);
        }
    }
}

/// Read the LAPIC ID of the *current* CPU directly from the local APIC register.
pub fn current_lapic_id() -> u8 {
    (lapic_read32(LAPIC_ID) >> 24) as u8
}

/// Read a 32-bit LAPIC register, dispatching to MMIO (xAPIC) or MSR (x2APIC).
///
/// # Safety contract
///
/// * `PHYSICAL_MEMORY_OFFSET` is installed by the memory subsystem before any
///   LAPIC register is touched (guaranteed during `apic::init`).
/// * `LAPIC_PHYS_BASE` is the fixed local-APIC MMIO base mandated by the
///   Intel SDM; accesses below are within the documented register window.
/// * Each CPU's bus access to `LAPIC_PHYS_BASE` targets that CPU's own
///   local APIC, so no locking is required.
pub fn lapic_read32(offset: u32) -> u32 {
    if is_x2apic() {
        x2apic_read(offset)
    } else {
        let pmo = crate::memory::physical_memory_offset();
        let ptr = (pmo + LAPIC_PHYS_BASE + offset as u64) as *const u32;
        unsafe { core::ptr::read_volatile(ptr) }
    }
}

/// Read a 32-bit local APIC register via x2APIC MSR.
///
/// x2APIC maps registers to MSRs using: MSR = 0x800 + (offset / 16).
/// Only the low 32 bits of each MSR are used.
fn x2apic_read(offset: u32) -> u32 {
    let msr = 0x800u32 + (offset >> 4);
    let mut val: u64;
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

/// Write a 32-bit LAPIC register of the current CPU.
///
/// This is a safe, lock-free helper: the `unsafe` is encapsulated here because
/// the target is a fixed, identity-mapped MMIO address guaranteed valid after
/// boot. Marking callers `unsafe` would force `unsafe` onto every interrupt
/// handler and SMP call site for no safety benefit.
/// Write a 32-bit LAPIC register, dispatching to MMIO (xAPIC) or MSR (x2APIC).
fn lapic_write32(offset: u32, value: u32) {
    if is_x2apic() {
        x2apic_write(offset, value)
    } else {
        let pmo = crate::memory::physical_memory_offset();
        let ptr = (pmo + LAPIC_PHYS_BASE + offset as u64) as *mut u32;
        unsafe { core::ptr::write_volatile(ptr, value); }
    }
}

/// Write a 32-bit local APIC register via x2APIC MSR.
fn x2apic_write(offset: u32, value: u32) {
    let msr = 0x800u32 + (offset >> 4);
    let val = value as u64;
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

/// Send an IPI to a specific LAPIC via ICR writes on the current CPU.
///
/// `delivery_mode` encodes the ICR delivery-mode field (e.g. 0 = Fixed, 4 =
/// Init, 5 = Startup). The destination is placed in ICR high.
pub fn send_ipi(dest_lapic_id: u8, vector: u8, delivery_mode: u8) {
    lapic_write32(ICR_HIGH, (dest_lapic_id as u32) << 24);
    lapic_write32(
        ICR_LOW,
        ICR_ASSERT | ((delivery_mode as u32) << ICR_DELIVERY_MODE_SHIFT) | vector as u32,
    );
}

/// Broadcast an IPI to all CPUs excluding the current one.
pub fn send_broadcast_ipi(vector: u8) {
    lapic_write32(ICR_HIGH, 0);
    lapic_write32(
        ICR_LOW,
        ICR_SHORTHAND_ALL_EXCL_SELF | ICR_ASSERT | vector as u32,
    );
}

/// Send a Non-Maskable Interrupt (NMI) to a specific CPU.
///
/// NMI delivery mode (4) cannot be masked by the TPR. Use sparingly.
pub fn send_nmi(dest_lapic_id: u8, vector: u8) {
    send_ipi(dest_lapic_id, vector, ICR_DELIVERY_MODE_NMI);
}

/// Send a System Management Interrupt (SMI) to a specific CPU.
pub fn send_smi(dest_lapic_id: u8) {
    send_ipi(dest_lapic_id, 0, ICR_DELIVERY_MODE_SMI);
}

/// Send a Performance Monitoring Interrupt (PMI) to a specific CPU.
///
/// PMI (delivery mode 15) is only available in x2APIC mode. On xAPIC
/// systems this falls back to a fixed-mode IPI.
pub fn send_pmi(dest_lapic_id: u8, vector: u8) {
    let mode = if is_x2apic() {
        ICR_DELIVERY_MODE_PMI
    } else {
        ICR_DELIVERY_MODE_FIXED
    };
    send_ipi(dest_lapic_id, vector, mode);
}

/// Send a lowest-priority IPI to distribute an interrupt across a group
/// of CPUs. The APIC hardware selects the target CPU.
pub fn send_lowest_priority(dest_lapic_id: u8, vector: u8) {
    send_ipi(dest_lapic_id, vector, 1);
}

/// Maximum spin iterations before declaring IPI delivery stalled.
const MAX_IPI_WAIT: u64 = 1_000_000;

/// Spin-wait until the current CPU's IPI delivery completes.
///
/// Polls the ICR Delivery Status bit (bit 12); clears when the ICR is no
/// longer busy. Returns `true` on success, `false` if the bounded window
/// elapsed — callers should treat `false` as a soft error rather than panic.
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

/// Signal end-of-interrupt to the current CPU's local APIC.
///
/// This is the hot-path EOI used by every interrupt handler. It writes
/// `0` to the EOI register of *this* CPU at the fixed LAPIC MMIO base — it
/// must remain lock-free.
pub fn eoi() {
    lapic_write32(LAPIC_EOI, 0);
}

/// Route a legacy PCI ISA IRQ line through the first I/O APIC that can
/// host it, using any ACPI interrupt override for polarity/trigger mode.
pub fn route_pci_irq(irq: u8, vector: u8) {
    let (active_low, level, gsi) = override_flags(irq);
    route_gsi(gsi, vector, active_low, level);
}

/// Route an interrupt by Global System Interrupt (GSI) directly.
///
/// Used for PCI INTx routing when the GSI is known (e.g., from ACPI _PRT
/// parsing). Falls back to ISA-bus defaults when no override covers this
/// GSI. Prefer this over `route_pci_irq` when the caller knows the GSI.
pub fn route_by_gsi(gsi: u8, vector: u8) {
    let (active_low, level) = override_flags_by_gsi(gsi);
    route_gsi(gsi, vector, active_low, level);
}

/// Common IOAPIC redirection helper. Iterates IOAPIC addresses and programs
/// the first one that can host `gsi`.
fn route_gsi(gsi: u8, vector: u8, active_low: bool, level: bool) {
    let id = current_lapic_id();
    if let Some(addrs) = crate::acpi::IOAPIC_ADDRS.get() {
        for &addr in addrs {
            let mut io = unsafe { ioapic::IoApic::new(addr) };
            if gsi <= io.max_redirection_entry() {
                io.set_redirection(gsi, vector, id, active_low, level, false);
                return;
            }
        }
    }
}
















