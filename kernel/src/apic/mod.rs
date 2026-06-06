pub mod lapic;
pub mod ioapic;

pub fn init() {
    // 1. Initialize Local APIC
    lapic::init();

    // 2. Initialize I/O APICs
    let lapic_id = lapic::LOCAL_APIC.lock()
            .as_ref()
            .expect("Local APIC not initialized")
            .id() as u8;
    
    if let Some(ioapic_addrs) = crate::acpi::IOAPIC_ADDRS.get() {
        for &addr in ioapic_addrs {
            let mut ioapic = unsafe { ioapic::IoApic::new(addr) };
            
            // Map legacy hardware interrupts through I/O APIC
            // IRQ 1: Keyboard -> Vector 33
            ioapic.set_redirection(1, 33, lapic_id, false);
            // IRQ 12: Mouse -> Vector 44
            ioapic.set_redirection(12, 44, lapic_id, false);
            
            crate::println!("I/O APIC: Initialized at 0x{:x}", addr);
        }
    }
}

pub fn eoi() {
    if let Some(ref mut lapic) = *lapic::LOCAL_APIC.lock() {
        lapic.eoi();
    }
}
