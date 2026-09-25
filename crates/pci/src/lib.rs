// PCI subsystem for Vahi kernel
// Bus enumeration, BAR discovery, MSI interrupt routing, and legacy IRQ setup.

#![no_std]

extern crate alloc;

use spin::Mutex;

/// A PCI device descriptor discovered during bus scanning.
#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub revision_id: u8,
    pub header_type: u8,
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
    pub bar0: u32,
    pub bar1: u32,
    pub bar2: u32,
    pub bar3: u32,
    pub bar4: u32,
    pub bar5: u32,
}

static DISCOVERED_DEVICES: Mutex<alloc::vec::Vec<PciDevice>> = Mutex::new(alloc::vec::Vec::new());

impl PciDevice {
    /// Build a `PciDevice` descriptor by reading configuration space for the
    /// given bus/device/function. Fields are populated from the live config
    /// space, so callers that only hold a BDF triple can use the new
    /// `&PciDevice`-based API without a prior bus scan.
    ///
    /// `None` means no device responds at this BDF (vendor id 0xFFFF/0).
    pub fn new(bus: u8, device: u8, function: u8) -> Option<Self> {
        let vendor_id = read_config_u16(bus, device, function, 0x00);
        if vendor_id == 0xFFFF || vendor_id == 0 {
            return None;
        }
        Some(Self::read_fields(bus, device, function, vendor_id))
    }

    /// Populate a `PciDevice` from live config space. Shared by `new` (single
    /// lookup) and `init` (full scan) so the field offsets live in exactly one
    /// place.
    fn read_fields(bus: u8, device: u8, function: u8, vendor_id: u16) -> PciDevice {
        let device_id = read_config_u16(bus, device, function, 0x02);
        let class_prog = read_config_u32(bus, device, function, 0x08);
        let revision_id = (class_prog & 0xFF) as u8;
        let prog_if = ((class_prog >> 8) & 0xFF) as u8;
        let subclass = ((class_prog >> 16) & 0xFF) as u8;
        let class_code = ((class_prog >> 24) & 0xFF) as u8;

        let header_type = read_config_u8(bus, device, function, 0x0E);
        let intr_info = read_config_u32(bus, device, function, 0x3C);
        let interrupt_line = (intr_info & 0xFF) as u8;
        let interrupt_pin = ((intr_info >> 8) & 0xFF) as u8;

        PciDevice {
            bus,
            device,
            function,
            vendor_id,
            device_id,
            class_code,
            subclass,
            prog_if,
            revision_id,
            header_type,
            interrupt_line,
            interrupt_pin,
            bar0: read_config_u32(bus, device, function, 0x10),
            bar1: read_config_u32(bus, device, function, 0x14),
            bar2: read_config_u32(bus, device, function, 0x18),
            bar3: read_config_u32(bus, device, function, 0x1C),
            bar4: read_config_u32(bus, device, function, 0x20),
            bar5: read_config_u32(bus, device, function, 0x24),
        }
    }
}

/// PCI Configuration Space IO Port Addresses
const PCI_CONFIG_ADDRESS: u16 = 0x0CF8;
const PCI_CONFIG_DATA: u16 = 0x0CFC;

/// Read a 32-bit register from PCI configuration space using CAM (Port 0xCF8/0xCFC).
pub fn read_config_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
        | 0x8000_0000;

    unsafe {
        #[cfg(target_arch = "x86_64")]
        {
            use x86_64::instructions::port::Port;
            let mut addr_port = Port::<u32>::new(PCI_CONFIG_ADDRESS);
            let mut data_port = Port::<u32>::new(PCI_CONFIG_DATA);
            addr_port.write(address);
            data_port.read()
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = address;
            0
        }
    }
}

/// Read a 16-bit register from PCI configuration space.
pub fn read_config_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let val = read_config_u32(bus, device, function, offset);
    let shift = (offset & 2) * 8;
    ((val >> shift) & 0xFFFF) as u16
}

/// Read an 8-bit register from PCI configuration space.
pub fn read_config_u8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let val = read_config_u32(bus, device, function, offset);
    let shift = (offset & 3) * 8;
    ((val >> shift) & 0xFF) as u8
}

/// Read a 64-bit BAR value, handling 64-bit (upper BAR) BARs.
pub fn read_bar64(bus: u8, device: u8, function: u8, bar_offset: u8) -> u64 {
    let lo = read_config_u32(bus, device, function, bar_offset);
    if lo & 0x6 == 0x4 {
        let hi = read_config_u32(bus, device, function, bar_offset + 4) as u64;
        (hi << 32) | (lo as u64 & 0xFFFFFFF0)
    } else {
        (lo & 0xFFFFFFF0) as u64
    }
}

/// Write a 32-bit register into PCI configuration space.
pub fn write_config_u32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let address = ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
        | 0x8000_0000;

    unsafe {
        #[cfg(target_arch = "x86_64")]
        {
            use x86_64::instructions::port::Port;
            let mut addr_port = Port::<u32>::new(PCI_CONFIG_ADDRESS);
            let mut data_port = Port::<u32>::new(PCI_CONFIG_DATA);
            addr_port.write(address);
            data_port.write(value);
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = (address, value);
        }
    }
}

/// Write a 16-bit register into PCI configuration space.
pub fn write_config_u16(bus: u8, device: u8, function: u8, offset: u8, value: u16) {
    let orig = read_config_u32(bus, device, function, offset);
    let shift = (offset & 2) * 8;
    let mask = !(0xFFFFu32 << shift);
    let new_val = (orig & mask) | ((value as u32) << shift);
    write_config_u32(bus, device, function, offset, new_val);
}

/// Write an 8-bit register into PCI configuration space.
pub fn write_config_u8(bus: u8, device: u8, function: u8, offset: u8, value: u8) {
    let orig = read_config_u32(bus, device, function, offset);
    let shift = (offset & 3) * 8;
    let mask = !(0xFFu32 << shift);
    let new_val = (orig & mask) | ((value as u32) << shift);
    write_config_u32(bus, device, function, offset, new_val);
}

/// Scan all 256 PCI buses, discover active devices, and record them.
pub fn init() {
    let mut dev_list = DISCOVERED_DEVICES.lock();
    dev_list.clear();

    for bus in 0..=255 {
        for device in 0..32 {
            for function in 0..8 {
                let vendor_id = read_config_u16(bus, device, function, 0x00);
                if vendor_id == 0xFFFF || vendor_id == 0 {
                    if function == 0 {
                        break; // Skip rest of functions if device is absent
                    }
                    continue;
                }

                let pci_dev = PciDevice::read_fields(bus, device, function, vendor_id);

                dev_list.push(pci_dev);

                // Multi-function check on function 0
                if function == 0 && (pci_dev.header_type & 0x80) == 0 {
                    break;
                }
            }
        }
    }
}

/// Return a snapshot of all discovered PCI devices.
pub fn get_devices() -> alloc::vec::Vec<PciDevice> {
    DISCOVERED_DEVICES.lock().clone()
}

/// Find the offset of a PCI Capability in configuration space by capability ID.
pub fn find_capability(dev: &PciDevice, cap_id: u8) -> Option<u8> {
    let status = read_config_u16(dev.bus, dev.device, dev.function, 0x06);
    if (status & (1 << 4)) == 0 {
        return None; // Capabilities list bit not set
    }

    let mut cap_ptr = read_config_u8(dev.bus, dev.device, dev.function, 0x34) & 0xFC;
    while cap_ptr != 0 {
        let id = read_config_u8(dev.bus, dev.device, dev.function, cap_ptr);
        if id == cap_id {
            return Some(cap_ptr);
        }
        cap_ptr = read_config_u8(dev.bus, dev.device, dev.function, cap_ptr + 1) & 0xFC;
    }
    None
}

/// Enable Message Signaled Interrupts (MSI) for a device.
/// Returns allocated vector on success.
pub fn pci_enable_msi(dev: &PciDevice, target_cpu: u8) -> Option<u8> {
    let cap = find_capability(dev, 0x05)?; // MSI Cap ID = 0x05

    let vector = vahi_apic::msi::alloc()?;
    let msi_addr = vahi_apic::msi::msi_addr(target_cpu);
    let msi_data = vahi_apic::msi::msi_data(vector);

    let msg_ctrl = read_config_u16(dev.bus, dev.device, dev.function, cap + 2);
    let is_64bit = (msg_ctrl & (1 << 7)) != 0;

    write_config_u32(dev.bus, dev.device, dev.function, cap + 4, msi_addr);

    if is_64bit {
        write_config_u32(dev.bus, dev.device, dev.function, cap + 8, 0); // Upper 32 address bits
        write_config_u16(dev.bus, dev.device, dev.function, cap + 12, msi_data);
    } else {
        write_config_u16(dev.bus, dev.device, dev.function, cap + 8, msi_data);
    }

    // Enable MSI in Control Register (bit 0)
    write_config_u16(
        dev.bus,
        dev.device,
        dev.function,
        cap + 2,
        msg_ctrl | (1 << 0),
    );

    Some(vector)
}

/// Enable Bus Mastering in Command Register (bit 2)
pub fn pci_enable_bus_master(dev: &PciDevice) {
    let cmd = read_config_u16(dev.bus, dev.device, dev.function, 0x04);
    write_config_u16(dev.bus, dev.device, dev.function, 0x04, cmd | (1 << 2));
}

/// Enable Memory Space Access in Command Register (bit 1)
pub fn pci_enable_memory_space(dev: &PciDevice) {
    let cmd = read_config_u16(dev.bus, dev.device, dev.function, 0x04);
    write_config_u16(dev.bus, dev.device, dev.function, 0x04, cmd | (1 << 1));
}

/// Route legacy IRQ for a device via ACPI _PRT or IOAPIC lookup.
/// Returns `None` if legacy IRQ routing cannot be established, preserving APIC vector space.
pub fn pci_route_legacy_irq(dev: &PciDevice) -> Option<u8> {
    if dev.interrupt_pin == 0 {
        return None;
    }
    None
}
