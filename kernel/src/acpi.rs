use acpi::{AcpiHandler, PhysicalMapping, AcpiTables};
use core::ptr::NonNull;
use crate::memory;
use crate::println;

#[derive(Clone, Copy)]
pub struct SkyAcpiHandler;

impl AcpiHandler for SkyAcpiHandler {
    unsafe fn map_physical_region<T>(&self, physical_address: usize, size: usize) -> PhysicalMapping<Self, T> {
        let offset = *memory::PHYSICAL_MEMORY_OFFSET.get()
            .expect("PHYSICAL_MEMORY_OFFSET must be initialized before ACPI");
        let virtual_address = offset + physical_address as u64;
        PhysicalMapping::new(
            physical_address,
            NonNull::new(virtual_address as *mut T).expect("null address for ACPI mapping"),
            size,
            size,
            Self,
        )
    }

    fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {}
}

pub static LAPIC_ADDR: spin::Once<usize> = spin::Once::new();
pub static IOAPIC_ADDRS: spin::Once<alloc::vec::Vec<usize>> = spin::Once::new();
pub static AP_LAPIC_IDS: spin::Once<alloc::vec::Vec<u8>> = spin::Once::new();

// ACPI power management info parsed from FADT
pub static PM1A_CNT_PORT: spin::Once<u16> = spin::Once::new();
pub static PM1B_CNT_PORT: spin::Once<Option<u16>> = spin::Once::new();
pub static RESET_REG_PORT: spin::Once<Option<(u16, u8)>> = spin::Once::new(); // (port, value)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    SameAsBus,
    ActiveHigh,
    ActiveLow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerMode {
    SameAsBus,
    Edge,
    Level,
}

pub struct InterruptOverride {
    pub _isa_irq: u8,
    pub _global_system_interrupt: u32,
    pub _polarity: Polarity,
    pub _trigger_mode: TriggerMode,
}

pub static OVERRIDES: spin::Once<alloc::vec::Vec<InterruptOverride>> = spin::Once::new();

pub fn init(boot_rsdp: Option<u64>) {
    let handler = SkyAcpiHandler;
    crate::serial_write("[ACPI] find_rsdp...\n");
    let rsdp_addr = match boot_rsdp.or_else(|| find_rsdp().map(|a| a as u64)) {
        Some(addr) => { crate::serial_write(&alloc::format!("[ACPI] RSDP at 0x{:x}\n", addr)); addr as usize }
        None => {
            println!("ERROR: Failed to find ACPI RSDP");
            crate::serial_write("[ACPI] FATAL: RSDP not found\n");
            return;
        }
    };

    crate::serial_write("[ACPI] loading tables...\n");
    let tables = unsafe {
        match AcpiTables::from_rsdp(handler, rsdp_addr) {
            Ok(t) => { crate::serial_write("[ACPI] tables loaded\n"); t }
            Err(e) => {
                println!("ERROR: Failed to load ACPI tables: {:?}", e);
                crate::serial_write(&alloc::format!("[ACPI] table error: {:?}\n", e));
                return;
            }
        }
    };

    println!("ACPI: Tables loaded at 0x{:x}.", rsdp_addr);

    // Parse FADT for power management registers
    parse_fadt(&tables);

    crate::serial_write("[ACPI] platform_info...\n");
    if let Ok(platform_info) = tables.platform_info() {
        crate::serial_write("[ACPI] got platform_info\n");
        if let acpi::platform::interrupt::InterruptModel::Apic(apic) = platform_info.interrupt_model {
            crate::serial_write(&alloc::format!("[ACPI] LAPIC addr=0x{:x}\n", apic.local_apic_address));
            LAPIC_ADDR.call_once(|| apic.local_apic_address as usize);
            println!("ACPI: LAPIC Address: 0x{:x}", apic.local_apic_address);

            let mut ioapics = alloc::vec::Vec::new();
            for ioapic in apic.io_apics.iter() {
                ioapics.push(ioapic.address as usize);
                println!("ACPI: I/O APIC Address: 0x{:x}", ioapic.address);
            }
            IOAPIC_ADDRS.call_once(|| ioapics);

            let mut overrides = alloc::vec::Vec::new();
            for interrupt_override in apic.interrupt_source_overrides.iter() {
                let pol = match interrupt_override.polarity {
                    acpi::platform::interrupt::Polarity::SameAsBus => Polarity::SameAsBus,
                    acpi::platform::interrupt::Polarity::ActiveHigh => Polarity::ActiveHigh,
                    acpi::platform::interrupt::Polarity::ActiveLow => Polarity::ActiveLow,
                };
                let trig = match interrupt_override.trigger_mode {
                    acpi::platform::interrupt::TriggerMode::SameAsBus => TriggerMode::SameAsBus,
                    acpi::platform::interrupt::TriggerMode::Edge => TriggerMode::Edge,
                    acpi::platform::interrupt::TriggerMode::Level => TriggerMode::Level,
                };
                overrides.push(InterruptOverride {
                    _isa_irq: interrupt_override.isa_source,
                    _global_system_interrupt: interrupt_override.global_system_interrupt,
                    _polarity: pol,
                    _trigger_mode: trig,
                });
            }
            OVERRIDES.call_once(|| overrides);
        } else {
            crate::serial_write("[ACPI] interrupt model is NOT Apic!\n");
        }

        if let Some(processor_info) = platform_info.processor_info {
            let mut ap_ids = alloc::vec::Vec::new();
            for ap in processor_info.application_processors.iter() {
                if ap.state == ::acpi::platform::ProcessorState::WaitingForSipi ||
                   ap.state == ::acpi::platform::ProcessorState::Running {
                    ap_ids.push(ap.local_apic_id as u8);
                }
            }
            let cpu_count = 1 + ap_ids.len();
            println!("ACPI: Total CPU cores detected: {}", cpu_count);
            AP_LAPIC_IDS.call_once(|| ap_ids);
        }
    } else {
        crate::serial_write("[ACPI] platform_info FAILED!\n");
    }
}

fn parse_fadt(tables: &AcpiTables<SkyAcpiHandler>) {
    use acpi::fadt::Fadt;
    use acpi::sdt::Signature;

    if let Ok(Some(fadt_mapping)) = unsafe { tables.get_sdt::<Fadt>(Signature::FADT) } {
        let fadt = &*fadt_mapping;

        // PM1a Control Block
        if let Ok(pm1a) = fadt.pm1a_control_block() {
            if pm1a.address_space == acpi::platform::address::AddressSpace::SystemIo {
                let port = pm1a.address as u16;
                PM1A_CNT_PORT.call_once(|| port);
                println!("ACPI: PM1a_CNT_BLK at port 0x{:x}", port);
            }
        }

        // PM1b Control Block (optional)
        if let Ok(Some(pm1b)) = fadt.pm1b_control_block() {
            if pm1b.address_space == acpi::platform::address::AddressSpace::SystemIo {
                let port = pm1b.address as u16;
                PM1B_CNT_PORT.call_once(|| Some(port));
                println!("ACPI: PM1b_CNT_BLK at port 0x{:x}", port);
            }
        } else {
            PM1B_CNT_PORT.call_once(|| None);
        }

        // Reset Register
        let fadt_flags = fadt.flags; // copy to aligned local
        if fadt_flags.supports_system_reset_via_fadt() {
            if let Ok(reset_reg) = fadt.reset_register() {
                let reset_val = fadt.reset_value;
                if reset_reg.address_space == acpi::platform::address::AddressSpace::SystemIo {
                    RESET_REG_PORT.call_once(|| Some((reset_reg.address as u16, reset_val)));
                    println!("ACPI: RESET_REG at port 0x{:x} (val 0x{:x})", reset_reg.address, reset_val);
                } else {
                    println!("ACPI: RESET_REG at non-I/O address space, skipping");
                    RESET_REG_PORT.call_once(|| None);
                }
            }
        } else {
            RESET_REG_PORT.call_once(|| None);
        }
    } else {
        println!("ACPI: FADT not found, power management unavailable");
        PM1A_CNT_PORT.call_once(|| 0);
        PM1B_CNT_PORT.call_once(|| None);
        RESET_REG_PORT.call_once(|| None);
    }
}

/// ACPI S5 (soft-off) shutdown via PM1a/PM1b control block.
pub fn acpi_shutdown() {
    let pm1a_port = match PM1A_CNT_PORT.get() {
        Some(&p) if p != 0 => p,
        _ => return,
    };

    // SLP_TYP for S5 = 0, SLP_EN = bit 13 (0x2000)
    let slp_typa: u16 = 0x0000; // SLP_TYP = 0 for S5
    let slp_en: u16 = 0x2000;   // SLP_EN
    let pm1a_val = slp_typa | slp_en;

    if let Some(Some(pm1b_port)) = PM1B_CNT_PORT.get() {
        let mut pm1b_port = x86_64::instructions::port::Port::<u16>::new(*pm1b_port);
        unsafe { pm1b_port.write(pm1a_val); }
    }

    let mut pm1a_port = x86_64::instructions::port::Port::<u16>::new(pm1a_port);
    unsafe { pm1a_port.write(pm1a_val); }

    // Wait for power loss
    x86_64::instructions::interrupts::disable();
    loop { x86_64::instructions::hlt(); }
}

/// ACPI system reset via RESET_REG.
pub fn acpi_reboot() {
    if let Some(&Some((port, reset_val))) = RESET_REG_PORT.get() {
        let mut port = x86_64::instructions::port::Port::<u8>::new(port);
        unsafe { port.write(reset_val); }
        x86_64::instructions::interrupts::disable();
        loop { x86_64::instructions::hlt(); }
    }

    // Fallback: legacy keyboard controller reset
    let mut kbc = x86_64::instructions::port::Port::<u8>::new(0x64);
    unsafe { kbc.write(0xFE); }
    x86_64::instructions::interrupts::disable();
    loop { x86_64::instructions::hlt(); }
}

fn find_rsdp() -> Option<usize> {
    let offset = *memory::PHYSICAL_MEMORY_OFFSET.get().unwrap();
    let ebda_ptr_virt = offset + 0x40E;
    let ebda_base = unsafe { (*(ebda_ptr_virt as *const u16) as u64) << 4 };

    if ebda_base > 0 {
        if let Some(addr) = search_range(offset + ebda_base, offset + ebda_base + 1024) {
            return Some((addr - offset) as usize);
        }
    }

    search_range(offset + 0xE0000, offset + 0x100000)
        .map(|addr| (addr - offset) as usize)
}

fn search_range(start: u64, end: u64) -> Option<u64> {
    for addr in (start..end).step_by(16) {
        let signature = unsafe { *(addr as *const [u8; 8]) };
        if &signature == b"RSD PTR " {
            return Some(addr);
        }
    }
    None
}
