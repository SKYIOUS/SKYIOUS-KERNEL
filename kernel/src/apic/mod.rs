pub mod lapic;
pub mod ioapic;
pub mod msi;

pub fn init() {
    lapic::init();
    msi::init();

    let lapic_id = current_lapic_id();

    if let Some(ioapic_addrs) = crate::acpi::IOAPIC_ADDRS.get() {
        for &addr in ioapic_addrs {
            let mut ioapic = unsafe { ioapic::IoApic::new(addr) };

            ioapic.set_redirection(1, 33, lapic_id, false);
            ioapic.set_redirection(12, 44, lapic_id, false);

            crate::println!("I/O APIC: Initialized at 0x{:x}", addr);
        }
    }
}

/// Read the LAPIC ID of the *current* CPU directly from the local APIC register.
/// This is inherently per-CPU (unlike the global LOCAL_APIC which the AP overwrites).
pub fn current_lapic_id() -> u8 {
    (lapic_read32(0x20) >> 24) as u8
}

/// Read a 32-bit LAPIC register of the current CPU.
pub fn lapic_read32(offset: u32) -> u32 {
    let pmo = crate::memory::physical_memory_offset();
    let ptr = (pmo + 0xfee00000 + offset as u64) as *const u32;
    // SAFETY: PHYSICAL_MEMORY_OFFSET is set during boot; LAPIC base is fixed.
    unsafe { core::ptr::read_volatile(ptr) }
}

/// Write a 32-bit LAPIC register of the current CPU.
unsafe fn lapic_write32(offset: u32, value: u32) {
    let pmo = crate::memory::physical_memory_offset();
    let ptr = (pmo + 0xfee00000 + offset as u64) as *mut u32;
    unsafe { core::ptr::write_volatile(ptr, value); }
}

/// Send an IPI to a specific LAPIC via ICR writes on the current CPU.
pub fn send_ipi(dest_lapic_id: u8, vector: u8, delivery_mode: u8) {
    // SAFETY: write to current CPU's LAPIC ICR registers (0x310 high, 0x300 low).
    unsafe {
        lapic_write32(0x310, (dest_lapic_id as u32) << 24);
        lapic_write32(0x300, (1 << 14) | ((delivery_mode as u32) << 8) | (vector as u32));
    }
}

/// Broadcast IPI to all CPUs excluding self.
pub fn send_broadcast_ipi(vector: u8) {
    // SAFETY: write to current CPU's LAPIC ICR with "All Excluding Self" shorthand.
    unsafe {
        lapic_write32(0x310, 0);
        lapic_write32(0x300, (0x3 << 18) | (1 << 14) | (vector as u32));
    }
}

/// Spin-wait for the current CPU's IPI delivery to complete.
pub fn wait_for_ipi() {
    while (lapic_read32(0x300) & (1 << 12)) != 0 {
        core::hint::spin_loop();
    }
}

pub fn eoi() {
    // Direct write to the current CPU's LAPIC EOI register.
    // LAPIC registers are accessed at their physical address (0xfee00000) via
    // the physical memory mapping.  Each CPU's access to this address targets
    // its own LAPIC, so this is inherently per-CPU and does NOT need the
    // shared global LOCAL_APIC (which the AP overwrites on SMP).
    let pmo = crate::memory::physical_memory_offset();
    let eoi = (pmo + 0xfee00000 + 0xb0) as *mut u32;
    // SAFETY: PHYSICAL_MEMORY_OFFSET is set during boot and the LAPIC is at
    // a fixed physical address.  Write 0 to the EOI register of *this* CPU.
    unsafe { core::ptr::write_volatile(eoi, 0); }
}

pub fn route_pci_irq(irq: u8, vector: u8) {
    let id = current_lapic_id();

    if let Some(addrs) = crate::acpi::IOAPIC_ADDRS.get() {
        for &addr in addrs {
            let mut io = unsafe { ioapic::IoApic::new(addr) };
            if irq <= io.max_redirection_entry() {
                io.set_redirection(irq, vector, id, false);
                return;
            }
        }
    }
}
