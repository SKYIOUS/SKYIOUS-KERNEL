use x86_64::instructions::port::Port;

pub fn read_config_u32(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    let address: u32 = ((bus as u32) << 16) | ((slot as u32) << 11) |
                       ((func as u32) << 8) | (offset as u32 & 0xFC) | 0x80000000;

    let mut config_addr = Port::new(0xCF8);
    let mut config_data: Port<u32> = Port::new(0xCFC);

    unsafe {
        config_addr.write(address);
        config_data.read()
    }
}

pub fn read_config_u16(bus: u8, slot: u8, func: u8, offset: u8) -> u16 {
    (read_config_u32(bus, slot, func, offset) >> ((offset & 2) * 8)) as u16
}

pub fn read_config_u8(bus: u8, slot: u8, func: u8, offset: u8) -> u8 {
    (read_config_u32(bus, slot, func, offset) >> ((offset & 3) * 8)) as u8
}

pub fn write_config_u32(bus: u8, slot: u8, func: u8, offset: u8, value: u32) {
    let address: u32 = ((bus as u32) << 16) | ((slot as u32) << 11) |
                       ((func as u32) << 8) | (offset as u32 & 0xFC) | 0x80000000;

    let mut config_addr = Port::new(0xCF8);
    let mut config_data: Port<u32> = Port::new(0xCFC);

    unsafe {
        config_addr.write(address);
        config_data.write(value);
    }
}

pub fn write_config_u16(bus: u8, slot: u8, func: u8, offset: u8, value: u16) {
    let shift = (offset & 2) * 8;
    let mask = 0xFFFFu32 << shift;
    let aligned = read_config_u32(bus, slot, func, offset);
    write_config_u32(bus, slot, func, offset, (aligned & !mask) | ((value as u32) << shift));
}

pub fn read_bar64(bus: u8, slot: u8, func: u8, bar_offset: u8) -> u64 {
    let lo = read_config_u32(bus, slot, func, bar_offset);
    if lo & 0x6 == 0x4 {
        let hi = read_config_u32(bus, slot, func, bar_offset + 4) as u64;
        (hi << 32) | (lo as u64 & 0xFFFFFFF0)
    } else {
        (lo & 0xFFFFFFF0) as u64
    }
}

fn bar_to_virt(bar_val: u64) -> usize {
    let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap_or(&0);
    (offset as u64 + bar_val) as usize
}

/// Walk PCI capabilities list, return offset of matching capability ID
pub fn find_capability(bus: u8, slot: u8, func: u8, cap_id: u8) -> Option<u8> {
    let status = read_config_u16(bus, slot, func, 0x06);
    if status & (1 << 4) == 0 {
        return None;
    }
    let mut offset = read_config_u8(bus, slot, func, 0x34);
    while offset != 0 {
        if read_config_u8(bus, slot, func, offset) == cap_id {
            return Some(offset);
        }
        offset = read_config_u8(bus, slot, func, offset + 1);
    }
    None
}

/// Enable MSI for a PCI function, returns the allocated vector
pub fn pci_enable_msi(bus: u8, slot: u8, func: u8) -> Option<u8> {
    let cap = find_capability(bus, slot, func, 0x05)?;
    let vector = crate::apic::msi::alloc()?;
    let lapic_id = crate::apic::lapic::LOCAL_APIC.lock()
        .as_ref().map(|l| l.id() as u8).unwrap_or(0);

    let msg_ctrl = read_config_u16(bus, slot, func, cap + 2);
    let is_64bit = (msg_ctrl & (1 << 7)) != 0;
    // ponytail: single message only (MME=0), per-vector masking untouched

    let addr = crate::apic::msi::msi_addr(lapic_id);
    let data = crate::apic::msi::msi_data(vector);

    write_config_u32(bus, slot, func, cap + 4, addr);
    if is_64bit {
        write_config_u32(bus, slot, func, cap + 8, 0);
        write_config_u16(bus, slot, func, cap + 0x0C, data);
    } else {
        write_config_u16(bus, slot, func, cap + 0x08, data);
    }
    // Enable MSI, keep MME=0 (single message), clear MMC bits
    write_config_u16(bus, slot, func, cap + 2, (msg_ctrl & !0x70) | 1);

    Some(vector)
}

/// Route a PCI device's legacy interrupt through the I/O APIC
fn pci_route_legacy_irq(_bus: u8, _slot: u8, _func: u8, irq: u8) -> Option<u8> {
    let vector = crate::apic::msi::alloc()?;
    crate::apic::route_pci_irq(irq, vector);
    Some(vector)
}

fn enumerate_bus_slot(bus: u8, slot: u8) {
    let vendor0 = read_config_u16(bus, slot, 0, 0);
    if vendor0 == 0xFFFF {
        return;
    }

    let header_type = read_config_u16(bus, slot, 0, 0x0C);
    let is_multi = (header_type >> 8) & 0x80 != 0;
    let max_func = if is_multi { 8u8 } else { 1u8 };

    for func in 0..max_func {
        let vendor_id = read_config_u16(bus, slot, func, 0);
        if vendor_id == 0xFFFF {
            if func == 0 { return; }
            continue;
        }
        let device_id = read_config_u16(bus, slot, func, 2);
        let class_full = read_config_u32(bus, slot, func, 8);
        let class_code = ((class_full >> 24) & 0xFF) as u8;
        let subclass = ((class_full >> 16) & 0xFF) as u8;
        let prog_if = ((class_full >> 8) & 0xFF) as u8;

        crate::serial_write(&alloc::format!("  PCI Device: {:02x}:{:02x}.{:x} Vendor:{:04x} Device:{:04x} Class:{:02x}.{:02x} (if:{:02x})\n",
            bus, slot, func, vendor_id, device_id, class_code, subclass, prog_if));

        let irq = (read_config_u32(bus, slot, func, 0x3C) & 0xFF) as u8;

        // NVMe
        if class_code == 0x01 && subclass == 0x08 && prog_if == 0x02 {
            crate::println!("    -> NVMe Controller detected!");
            let bar0 = read_bar64(bus, slot, func, 0x10);
            crate::drivers::storage::nvme::NvmeController::new(bar_to_virt(bar0));
        }

        // AHCI/SATA
        if class_code == 0x01 && subclass == 0x06 {
            crate::println!("    -> AHCI/SATA Controller detected!");
            let bar5 = read_bar64(bus, slot, func, 0x24);
            let virt_abar = bar_to_virt(bar5);
            crate::println!("       ABAR: 0x{:x}", bar5);
            crate::drivers::storage::ahci::init(virt_abar);
        }

        // E1000
        if vendor_id == 0x8086 && device_id == 0x100E {
             crate::println!("    -> Intel E1000 Network Card detected!");

             let bar0 = read_bar64(bus, slot, func, 0x10);
             let mem_base = bar_to_virt(bar0);

             // Try MSI first; fall back to IOAPIC routing for legacy INTx#
             let net_vector = pci_enable_msi(bus, slot, func).unwrap_or_else(|| {
                 pci_route_legacy_irq(bus, slot, func, irq)
                     .expect("no available vectors for E1000 interrupt")
             });

             crate::interrupts::set_network_vector(net_vector);
             crate::println!("       Mem Base: 0x{:x}, IRQ: {}, Vector: {}", bar0, irq, net_vector);

             unsafe {
                 let mut nic_inner = crate::drivers::net::e1000::E1000::new(mem_base);
                 nic_inner.set_irq(irq);
                 nic_inner.init();

                 let nic_device = crate::drivers::net::e1000::E1000Device { inner: nic_inner };
                 let nic_arc = alloc::sync::Arc::new(spin::Mutex::new(nic_device));

                 *crate::drivers::net::NIC.lock() = Some(crate::drivers::net::NicDevice::E1000(nic_arc));
             }
         }

        // VirtIO-Block
        if vendor_id == 0x1AF4 && device_id == 0x1001 {
            crate::println!("    -> VirtIO-Block Device detected!");
            let bar0 = read_config_u32(bus, slot, func, 0x10);
            if bar0 & 1 != 0 {
                let io_base = (bar0 & 0xFFFFFFFC) as u16;
                crate::println!("       I/O Base: 0x{:x}", io_base);
                crate::drivers::storage::virtio_block::init(io_base);
            }
        }

        // VirtIO-GPU
        if vendor_id == 0x1AF4 && device_id == 0x1050 {
            crate::println!("    -> VirtIO-GPU Device detected!");
            let bar0 = read_config_u32(bus, slot, func, 0x10);
            if bar0 & 1 != 0 {
                let io_base = (bar0 & 0xFFFFFFFC) as u16;
                crate::println!("       I/O Base: 0x{:x}", io_base);
                crate::drivers::gpu::virtio_gpu::init(io_base);
            }
        }

        // VirtIO-Net
        if vendor_id == 0x1AF4 && device_id == 0x1000 {
            crate::println!("    -> VirtIO-Net Device detected!");
            let bar0 = read_config_u32(bus, slot, func, 0x10);
            if bar0 & 1 != 0 {
                let io_base = (bar0 & 0xFFFFFFFC) as u16;
                crate::println!("       I/O Base: 0x{:x}", io_base);

                let nic_inner = crate::drivers::net::virtio::VirtIONet::new(io_base);
                let nic_device = crate::drivers::net::virtio::VirtIONetDevice {
                    inner: alloc::sync::Arc::new(spin::Mutex::new(nic_inner))
                };
                let nic_arc = alloc::sync::Arc::new(spin::Mutex::new(nic_device));

                *crate::drivers::net::NIC.lock() = Some(crate::drivers::net::NicDevice::VirtIO(nic_arc));
            }
        }

        // BGA framebuffer
        if (vendor_id == 0x1234 && device_id == 0x1111) || (vendor_id == 0x80ee && device_id == 0xbeef) {
             let bar0 = read_config_u32(bus, slot, func, 0x10);
             let fb_phys = (bar0 & 0xFFFFFFF0) as usize;
             let bga = crate::drivers::graphics::bga::Bga::new(fb_phys);
             bga.init();
        }

        // Audio (class 0x04)
        if class_code == 0x04 {
            crate::serial_write("[PCI] Audio device detected!\n");
            crate::println!("    -> Audio Device detected!");
            if subclass == 0x01 || subclass == 0x03 {
                crate::println!("       -> Intel HDA Controller");
                let bar0 = read_bar64(bus, slot, func, 0x10);
                let virt_base = bar_to_virt(bar0);
                let mut hda = crate::drivers::audio::hda::HdaController::new(virt_base);
                hda.init();
                crate::drivers::audio::register_hda(hda);
            }
        }

        // XHCI (USB 3.0)
        if class_code == 0x0C && subclass == 0x03 && prog_if == 0x30 {
            crate::println!("    -> XHCI (USB 3.0) Controller detected!");
            let bar0 = read_bar64(bus, slot, func, 0x10);
            let virt_base = bar_to_virt(bar0);
            let mut xhci = crate::drivers::usb::xhci::XhciController::new(virt_base);
            xhci.init();
        }

        // UHCI (USB 1.x) — I/O BAR, bit 0 = 1
        #[cfg(feature = "uhci")]
        if class_code == 0x0C && subclass == 0x03 && prog_if == 0x00 {
            crate::println!("    -> UHCI (USB 1.x) Controller detected!");
            let bar0 = read_config_u32(bus, slot, func, 0x10);
            if bar0 & 1 != 0 {
                let io_base = (bar0 & 0xFFFC) as u16;
                crate::println!("       I/O Base: 0x{:x}", io_base);
                let mut uhci = crate::drivers::usb::uhci::UhciController::new(io_base);
                uhci.init();
            }
        }
        // EHCI (USB 2.0)
        if class_code == 0x0C && subclass == 0x03 && prog_if == 0x20 {
            crate::println!("    -> EHCI (USB 2.0) Controller detected! (not yet implemented)");
        }
    }
}

pub fn enumerate_pci() {
    crate::println!("PCI: Enumerating Bus...");
    for bus in 0..255u8 {
        for slot in 0..32u8 {
            enumerate_bus_slot(bus, slot);
        }
    }
}
