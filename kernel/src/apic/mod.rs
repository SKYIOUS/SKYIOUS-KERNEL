pub mod lapic;
pub mod ioapic;
pub mod msi;

pub fn init() {
    lapic::init();
    msi::init();

    let lapic_id = lapic::LOCAL_APIC.lock()
        .as_ref()
        .expect("Local APIC not initialized")
        .id() as u8;

    if let Some(ioapic_addrs) = crate::acpi::IOAPIC_ADDRS.get() {
        for &addr in ioapic_addrs {
            let mut ioapic = unsafe { ioapic::IoApic::new(addr) };

            ioapic.set_redirection(1, 33, lapic_id, false);
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

pub fn route_pci_irq(irq: u8, vector: u8) {
    if let Some(addrs) = crate::acpi::IOAPIC_ADDRS.get() {
        if let Some(ref lapic) = *lapic::LOCAL_APIC.lock() {
            let id = lapic.id() as u8;
            for &addr in addrs {
                let mut io = unsafe { ioapic::IoApic::new(addr) };
                if irq <= io.max_redirection_entry() {
                    io.set_redirection(irq, vector, id, false);
                    return;
                }
            }
        }
    }
}
