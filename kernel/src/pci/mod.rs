use x86_64::instructions::port::Port;

pub fn read_config_u16(bus: u8, slot: u8, func: u8, offset: u8) -> u16 {
    let address: u32 = ((bus as u32) << 16) | ((slot as u32) << 11) |
                       ((func as u32) << 8) | (offset as u32 & 0xFC) | 0x80000000;
    
    let mut config_addr = Port::new(0xCF8);
    let mut config_data: Port<u32> = Port::new(0xCFC);
    
    unsafe {
        config_addr.write(address);
        (config_data.read() >> ((offset & 2) * 8)) as u16
    }
}

/// Reads a 32-bit dword from PCI configuration space.
/// Offset must be 32-bit aligned.
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

/// Reads a 64-bit memory-mapped BAR, handling both 32-bit and 64-bit BAR types.
/// `bar_offset` must be the byte offset of the BAR register (e.g. 0x10, 0x14, ..., 0x24).
pub fn read_bar64(bus: u8, slot: u8, func: u8, bar_offset: u8) -> u64 {
    let lo = read_config_u32(bus, slot, func, bar_offset);
    // bit 0 = 0 ⇒ memory BAR; bits [2:1] = 10 ⇒ 64-bit
    if lo & 0x6 == 0x4 {
        let hi = read_config_u32(bus, slot, func, bar_offset + 4) as u64;
        (hi << 32) | (lo as u64 & 0xFFFFFFF0)
    } else {
        (lo & 0xFFFFFFF0) as u64
    }
}

/// Returns the virtual address for a PCI memory-mapped BAR by adding the physical memory offset.
fn bar_to_virt(bar_val: u64) -> usize {
    let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap_or(&0);
    (offset as u64 + bar_val) as usize
}

fn enumerate_bus_slot(bus: u8, slot: u8) {
    // Scan all functions 0-7; function 0 must exist if the slot is occupied
    let vendor0 = read_config_u16(bus, slot, 0, 0);
    if vendor0 == 0xFFFF {
        return;
    }

    // Determine number of functions from header type of function 0
    let header_type = read_config_u16(bus, slot, 0, 0x0C);
    let is_multi = (header_type >> 8) & 0x80 != 0;
    let max_func = if is_multi { 8u8 } else { 1u8 };

    for func in 0..max_func {
        let vendor_id = read_config_u16(bus, slot, func, 0);
        if vendor_id == 0xFFFF {
            if func == 0 {
                return; // function 0 missing, nothing to scan
            }
            continue;
        }
        let device_id = read_config_u16(bus, slot, func, 2);
        let class_full = read_config_u32(bus, slot, func, 8);
        let class_code = ((class_full >> 24) & 0xFF) as u8;
        let subclass = ((class_full >> 16) & 0xFF) as u8;
        let prog_if = ((class_full >> 8) & 0xFF) as u8;
        
        crate::serial_write(&alloc::format!("  PCI Device: {:02x}:{:02x}.{:x} Vendor:{:04x} Device:{:04x} Class:{:02x}.{:02x} (if:{:02x})\n",
            bus, slot, func, vendor_id, device_id, class_code, subclass, prog_if));
        
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
             
             let irq = (read_config_u32(bus, slot, func, 0x3C) & 0xFF) as u8;
             crate::println!("       Mem Base: 0x{:x}, IRQ: {}", bar0, irq);
             
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
