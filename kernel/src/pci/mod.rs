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

pub fn enumerate_pci() {
    crate::println!("PCI: Enumerating Bus...");
    for bus in 0..255 {
        for slot in 0..32 {
            let vendor_id = read_config_u16(bus as u8, slot as u8, 0, 0);
            if vendor_id != 0xFFFF {
                let device_id = read_config_u16(bus as u8, slot as u8, 0, 2);
                let class_full = read_config_u32(bus as u8, slot as u8, 0, 8);
                let class_code = ((class_full >> 24) & 0xFF) as u8;
                let subclass = ((class_full >> 16) & 0xFF) as u8;
                
                crate::serial_write(&alloc::format!("  PCI Device: {:02x}:{:02x}.0 Vendor:{:04x} Device:{:04x} Class:{:02x}.{:02x}\n",
                    bus, slot, vendor_id, device_id, class_code, subclass));
                
                if class_code == 0x01 && subclass == 0x08 {
                    let prog_if = ((class_full >> 8) & 0xFF) as u8;
                    if prog_if == 0x02 {
                        crate::println!("    -> NVMe Controller detected!");
                        let bar0 = read_config_u32(bus as u8, slot as u8, 0, 0x10);
                        let base_addr = (bar0 & 0xFFFFFFF0) as usize;
                        let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap_or(&0);
                        let virt_base = offset as usize + base_addr;
                        crate::drivers::storage::nvme::NvmeController::new(virt_base);
                    }
                }

                if class_code == 0x01 && subclass == 0x06 {
                    crate::println!("    -> AHCI/SATA Controller detected!");
                    
                    let bar5 = read_config_u32(bus as u8, slot as u8, 0, 0x24);
                    
                    // Mask out flag bits (lower 4 bits usually) to get base address
                    // For AHCI, it's a memory BAR.
                    let abar = (bar5 & 0xFFFFFFF0) as usize;
                    
                    crate::println!("       ABAR: 0x{:x}", abar);
                    
                    // Important: We need to map this in a real kernel if it's outside our identity map.
                    // For Vahi Phase 11, we assume we can access it via the physical offset if we adjust it.
                    // But `ahci::init` expects a mapped address.
                    // Let's pass the raw physical address and let ahci::init handle the offset logic if needed,
                    // but `ahci::init` casts it to a pointer. 
                    // We need to add PHYSICAL_MEMORY_OFFSET.
                    // We'll trust the driver to handle the pointer arithmetic or mapping.
                    // Wait, `ahci::init` uses `base_addr as *mut HbaMemory`. 
                    // So we should pass the VIRTUAL address.
                    
                    let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap_or(&0);
                    let virt_abar = offset as usize + abar;
                    
                    crate::drivers::storage::ahci::init(virt_abar);
                }
                
                // Check for E1000 (Intel 82540EM)
                if vendor_id == 0x8086 && device_id == 0x100E {
                     crate::println!("    -> Intel E1000 Network Card detected!");
                     
                     let bar0 = read_config_u32(bus as u8, slot as u8, 0, 0x10);
                     
                     let mem_base = (bar0 & 0xFFFFFFF0) as usize;
                     
                      let irq = (read_config_u32(bus as u8, slot as u8, 0, 0x3C) & 0xFF) as u8;
                      crate::println!("       Mem Base: 0x{:x}, IRQ: {}", mem_base, irq);
                      
                      let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap_or(&0);
                      let virt_base = offset as usize + mem_base;
                      
                      unsafe {
                          let mut nic_inner = crate::drivers::net::e1000::E1000::new(virt_base);
                          nic_inner.set_irq(irq);
                          nic_inner.init();
                          
                          let nic_device = crate::drivers::net::e1000::E1000Device { inner: nic_inner };
                          let nic_arc = alloc::sync::Arc::new(spin::Mutex::new(nic_device));
                          
                          *crate::drivers::net::NIC.lock() = Some(crate::drivers::net::NicDevice::E1000(nic_arc));
                      }
                 }

                // Check for VirtIO-Block
                if vendor_id == 0x1AF4 && device_id == 0x1001 {
                    crate::println!("    -> VirtIO-Block Device detected!");
                    let bar0 = read_config_u32(bus as u8, slot as u8, 0, 0x10);
                    if bar0 & 1 != 0 {
                        let io_base = (bar0 & 0xFFFFFFFC) as u16;
                        crate::println!("       I/O Base: 0x{:x}", io_base);
                        crate::drivers::storage::virtio_block::init(io_base);
                    }
                }

                // Check for VirtIO-GPU
                if vendor_id == 0x1AF4 && device_id == 0x1050 {
                    crate::println!("    -> VirtIO-GPU Device detected!");
                    let bar0 = read_config_u32(bus as u8, slot as u8, 0, 0x10);
                    if bar0 & 1 != 0 {
                        let io_base = (bar0 & 0xFFFFFFFC) as u16;
                        crate::println!("       I/O Base: 0x{:x}", io_base);
                        crate::drivers::gpu::virtio_gpu::init(io_base);
                    }
                }

                // Check for VirtIO-Net
                if vendor_id == 0x1AF4 && device_id == 0x1000 {
                    crate::println!("    -> VirtIO-Net Device detected!");
                    
                    let bar0 = read_config_u32(bus as u8, slot as u8, 0, 0x10);
                    
                    // Legacy VirtIO-Net uses I/O space for BAR0
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

                // Check for AMD PCnet (0x1022:0x2000)
                if vendor_id == 0x1022 && device_id == 0x2000 {
                    crate::println!("    -> AMD PCnet (Am79C973) detected! (Driver Pending)");
                    // We can add a PCnet driver later if needed, but for now just notify the user.
                    // This explains why E1000 isn't showing up if VirtualBox defaults to PCnet.
                }

                // Configure BGA display to match UEFI framebuffer resolution
                if (vendor_id == 0x1234 && device_id == 0x1111) || (vendor_id == 0x80ee && device_id == 0xbeef) {
                     let bar0 = read_config_u32(bus as u8, slot as u8, 0, 0x10);
                     let fb_phys = (bar0 & 0xFFFFFFF0) as usize;
                     let bga = crate::drivers::graphics::bga::Bga::new(fb_phys);
                     bga.init();
                }

                // Check for Audio devices (class 0x04)
                if class_code == 0x04 {
                    crate::serial_write("[PCI] Audio device detected!\n");
                    crate::println!("    -> Audio Device detected!");
                    if subclass == 0x01 || subclass == 0x03 {
                        crate::println!("       -> Intel HDA Controller");
                        let bar0 = read_config_u32(bus as u8, slot as u8, 0, 0x10);
                        let base_addr = (bar0 & 0xFFFFFFF0) as usize;
                        let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap_or(&0);
                        let virt_base = offset as usize + base_addr;
                        let mut hda = crate::drivers::audio::hda::HdaController::new(virt_base);
                        hda.init();
                        crate::drivers::audio::register_hda(hda);
                    }
                }

                // Check for XHCI (USB 3.0) Controller
                if class_code == 0x0C && subclass == 0x03 {
                    let prog_if = (read_config_u16(bus as u8, slot as u8, 0, 8) >> 8) as u8;
                    if prog_if == 0x30 {
                        crate::println!("    -> XHCI (USB 3.0) Controller detected!");
                        
                        let bar0 = read_config_u32(bus as u8, slot as u8, 0, 0x10);
                        let base_addr = (bar0 & 0xFFFFFFF0) as usize;
                        
                        let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap_or(&0);
                        let virt_base = offset as usize + base_addr;
                        
                        let mut xhci = crate::drivers::usb::xhci::XhciController::new(virt_base);
                        xhci.init();
                    }
                }
            }
        }
    }
}
