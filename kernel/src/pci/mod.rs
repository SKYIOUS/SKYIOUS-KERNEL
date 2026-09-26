//! PCI bus enumeration and device initialization.
//!
//! Pure config-space access is provided by `vahi-pci`.
//! This module owns the boot-path enumeration that initializes drivers.

pub use vahi_pci::*;

use crate::apic::msi;
use vahi_limine::hhdm_offset;

fn bar_to_virt(bar_val: u64) -> usize {
    let offset = crate::memory::physical_memory_offset();
    (offset + bar_val) as usize
}

#[cfg(not(target_arch = "aarch64"))]
pub fn map_bar_mmio(bar_phys: u64) {
    use x86_64::structures::paging::{Mapper, Page, PageTableFlags, Size4KiB};
    use x86_64::VirtAddr;
    let hhdm = hhdm_offset();
    let phys_offset = VirtAddr::new(hhdm);
    let level4 = unsafe { crate::memory::active_level_4_table(phys_offset) };
    let mut mapper =
        unsafe { x86_64::structures::paging::OffsetPageTable::new(level4, phys_offset) };
    let mut frame_allocator = crate::memory::buddy::BuddyFrameAllocator;
    let size = 256 * 1024u64;
    let start = bar_phys & !0xFFF;
    let end = (bar_phys + size + 0xFFF) & !0xFFF;
    let mut addr = start;
    while addr < end {
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(hhdm + addr));
        let frame =
            x86_64::structures::paging::PhysFrame::containing_address(x86_64::PhysAddr::new(addr));
        unsafe {
            let _ = mapper.map_to(
                page,
                frame,
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE,
                &mut frame_allocator,
            );
        }
        addr += 4096;
    }
}

pub fn pci_enable_msi(bus: u8, slot: u8, func: u8) -> Option<u8> {
    // The root kernel apic stack is `crate::apic` (inited in init_devices()
    // before PCI enumerate); `vahi_apc` is not yet wired into the boot path,
    // so its MemoryProvider/MEM is uninitialized and `vahi_apc::msi`/`current_lapic_id`
    // panic. Allocate + target via the local stack to stay consistent with every
    // other interrupt caller. `vahi_pci` is still used for pure config-space reads.
    let dev = PciDevice::new(bus, slot, func)?;
    let cap = find_capability(&dev, 0x05)?;
    let vector = msi::alloc()?;
    let lapic_id = crate::apic::current_lapic_id();
    let msg_ctrl = read_config_u16(bus, slot, func, cap + 2);
    let is_64bit = (msg_ctrl & (1 << 7)) != 0;
    let addr = msi::msi_addr(lapic_id);
    let data = msi::msi_data(vector);
    write_config_u32(bus, slot, func, cap + 4, addr);
    if is_64bit {
        write_config_u32(bus, slot, func, cap + 8, 0);
        write_config_u16(bus, slot, func, cap + 0x0C, data);
    } else {
        write_config_u16(bus, slot, func, cap + 0x08, data);
    }
    write_config_u16(bus, slot, func, cap + 2, (msg_ctrl & !0x70) | 1);
    Some(vector)
}

pub fn pci_route_legacy_irq(_bus: u8, _slot: u8, _func: u8, pin: u8) -> Option<u8> {
    let vector = msi::alloc()?;
    crate::apic::route_pci_irq(pin, vector);
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
            if func == 0 {
                return;
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

        let irq = (read_config_u32(bus, slot, func, 0x3C) & 0xFF) as u8;

        if class_code == 0x01 && subclass == 0x08 && prog_if == 0x02 {
            crate::println!("    -> NVMe Controller detected!");
            let bar0 = read_bar64(bus, slot, func, 0x10);
            crate::drivers::storage::nvme::NvmeController::new(bar_to_virt(bar0), bus, slot, func);
        }

        if class_code == 0x01 && subclass == 0x06 {
            crate::println!("    -> AHCI/SATA Controller detected!");
            let bar5 = read_bar64(bus, slot, func, 0x24);
            let virt_abar = bar_to_virt(bar5);
            crate::println!("       ABAR: 0x{:x}", bar5);
            crate::drivers::storage::ahci::init(virt_abar);
        }

        if class_code == 0x01 && subclass == 0x01 {
            crate::println!("    -> PATA/IDE Controller detected, using PIO fallback.");
            crate::drivers::storage::pata::init();
        }

        if vendor_id == 0x8086 && device_id == 0x100E {
            crate::println!("    -> Intel E1000 Network Card detected!");

            let bar0 = read_bar64(bus, slot, func, 0x10);
            #[cfg(not(target_arch = "aarch64"))]
            map_bar_mmio(bar0);
            let mem_base = bar_to_virt(bar0);

            let net_vector = pci_enable_msi(bus, slot, func)
                .or_else(|| pci_route_legacy_irq(bus, slot, func, irq));
            let net_vector = match net_vector {
                Some(v) => v,
                None => {
                    crate::println!("       E1000: no available interrupt vectors, skipping");
                    continue;
                }
            };

            crate::interrupts::set_network_vector(net_vector);
            crate::println!(
                "       Mem Base: 0x{:x}, IRQ: {}, Vector: {}",
                bar0,
                irq,
                net_vector
            );

            unsafe {
                let bdf = ((bus as u16) << 8) | ((slot as u16) << 3) | (func as u16);
                let mut nic_inner = crate::drivers::net::e1000::E1000::new(mem_base, bdf);
                nic_inner.set_irq(irq);
                nic_inner.init();

                let nic_device = crate::drivers::net::e1000::E1000Device { inner: nic_inner };
                let nic_arc = alloc::sync::Arc::new(crate::sync::IrqSafeMutex::new(nic_device));

                *crate::drivers::net::NIC.lock() =
                    Some(crate::drivers::net::NicDevice::E1000(nic_arc));
            }
        }

        if vendor_id == 0x1AF4 && device_id == 0x1001 {
            crate::println!("    -> VirtIO-Block Device detected!");
            let bar0 = read_config_u32(bus, slot, func, 0x10);
            if bar0 & 1 != 0 {
                let io_base = (bar0 & 0xFFFFFFFC) as u16;
                crate::println!("       I/O Base: 0x{:x}", io_base);
                crate::drivers::storage::virtio_block::init(io_base);
            }
        }

        if vendor_id == 0x1AF4 && device_id == 0x1050 {
            crate::println!("    -> VirtIO-GPU Device detected!");
            let bar0 = read_config_u32(bus, slot, func, 0x10);
            if bar0 & 1 != 0 {
                let io_base = (bar0 & 0xFFFFFFFC) as u16;
                crate::println!("       I/O Base: 0x{:x}", io_base);
                crate::drivers::gpu::virtio_gpu::init(io_base);
            }
        }

        if vendor_id == 0x1AF4 && device_id == 0x1000 {
            crate::println!("    -> VirtIO-Net Device detected!");
            let bar0 = read_config_u32(bus, slot, func, 0x10);
            if bar0 & 1 != 0 {
                let io_base = (bar0 & 0xFFFFFFFC) as u16;
                crate::println!("       I/O Base: 0x{:x}", io_base);

                let nic_inner = crate::drivers::net::virtio::VirtIONet::new(io_base);
                let nic_device = crate::drivers::net::virtio::VirtIONetDevice {
                    inner: alloc::sync::Arc::new(crate::sync::IrqSafeMutex::new(nic_inner)),
                };
                let nic_arc = alloc::sync::Arc::new(crate::sync::IrqSafeMutex::new(nic_device));

                *crate::drivers::net::NIC.lock() =
                    Some(crate::drivers::net::NicDevice::VirtIO(nic_arc));
            }
        }

        if (vendor_id == 0x1234 && device_id == 0x1111)
            || (vendor_id == 0x80ee && device_id == 0xbeef)
        {
            let bar0 = read_config_u32(bus, slot, func, 0x10);
            let fb_phys = (bar0 & 0xFFFFFFF0) as usize;
            let bga = crate::drivers::graphics::bga::Bga::new(fb_phys);
            bga.init();
        }

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

        if class_code == 0x0C && subclass == 0x03 && prog_if == 0x30 {
            crate::println!("    -> XHCI (USB 3.0) Controller detected!");
            let bar0 = read_bar64(bus, slot, func, 0x10);
            let virt_base = bar_to_virt(bar0);
            let bdf = ((bus as u16) << 8) | ((slot as u16) << 3) | (func as u16);
            let mut xhci = crate::drivers::usb::xhci::XhciController::new(virt_base, bdf);
            xhci.init();
            crate::drivers::usb::register_xhci(xhci);
        }

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
