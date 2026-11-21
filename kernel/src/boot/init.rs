//! Device enumeration and subsystem initialization phases.
//!
//! Extracted from kernel_main() to keep that function focused on orchestration.
//! Each function performs a sequential block of init calls; order is critical
//! and must not be changed.

use crate::memory::buddy::BuddyFrameAllocator;
use x86_64::structures::paging::OffsetPageTable;

/// Map a physical address range into the HHDM via the active page tables.
/// ponytail: factored out to deduplicate the 3 identical mapping loops in boot.
#[cfg(not(target_arch = "aarch64"))]
unsafe fn map_phys_range(
    mapper: &mut OffsetPageTable<'static>,
    frame_allocator: &mut BuddyFrameAllocator,
    phys_start: u64,
    phys_end: u64,
) {
    use x86_64::structures::paging::{Mapper, Page, PageTableFlags, Size4KiB};
    use x86_64::VirtAddr;
    let hhdm = crate::limine::hhdm_offset();
    let mut addr = phys_start & !0xFFF;
    let end = (phys_end + 0xFFF) & !0xFFF;
    while addr < end {
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(hhdm + addr));
        let frame =
            x86_64::structures::paging::PhysFrame::containing_address(x86_64::PhysAddr::new(addr));
        let _ = mapper.map_to(
            page,
            frame,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
            frame_allocator,
        );
        addr += 4096;
    }
}

/// Initialize memory subsystem: mapper, frame allocator, heap, HHDM mapping,
/// and framebuffer pages. Returns the mapper and frame allocator for any
/// remaining mapping work.
///
/// # Safety
/// Requires Limine memory map and HHDM to be valid.
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn init_memory(
    phys_mem_offset: x86_64::VirtAddr,
) -> (OffsetPageTable<'static>, BuddyFrameAllocator) {
    let mut mapper = unsafe { crate::memory::init(phys_mem_offset) };
    unsafe { crate::memory::init_frame_allocator_limine() };
    let mut frame_allocator = BuddyFrameAllocator;
    crate::allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");

    // Map all physical RAM into HHDM + known MMIO regions.
    for entry in crate::limine::memory_map() {
        if entry.length == 0 {
            continue;
        }
        map_phys_range(
            &mut mapper,
            &mut frame_allocator,
            entry.base,
            entry.base + entry.length,
        );
    }
    for &base in &[0xFEC0_0000u64, 0xFED0_0000, 0xfee00000] {
        map_phys_range(&mut mapper, &mut frame_allocator, base, base + 0x1000);
    }
    // T-00 (FB-002): the classic VGA text buffer at physical 0xB8000 sits in
    // the hole between Limine's conventional-RAM and extended-RAM entries
    // (0x9FC00..0x100000 is reserved/ACPI-reclaimed, not "usable"). It is
    // therefore never mapped into the HHDM by the loop above, and any write
    // through VGA_BUFFER_VIRT (vga_buffer::init -> clear_screen) faults before
    // the IDT exists: triple fault, silent reset. Whether the page happens to
    // be mapped is a function of the exact QEMU/OVMF memmap, which is why some
    // environments booted fine and others died silently after the splash.
    // Map the two frames covering 0xB0000..0xBFFFF explicitly — the region is
    // MMIO (present in every x86 PC), so the map is unconditional and safe.
    map_phys_range(&mut mapper, &mut frame_allocator, 0xB_0000, 0xC_0000);
    crate::serial_write("[BOOT] HHDM mapping done\n");

    (mapper, frame_allocator)
}

/// Map framebuffer pages and initialize graphics subsystem.
///
/// # Safety
/// Maps the framebuffer physical memory into the kernel virtual address space.
/// `mapper` and `frame_allocator` must be valid and the framebuffer address
/// must come from a trusted Limine boot info structure.
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn init_graphics(
    mapper: &mut OffsetPageTable<'static>,
    frame_allocator: &mut BuddyFrameAllocator,
) {
    let hhdm = crate::limine::hhdm_offset();
    let fb = crate::limine::framebuffer();
    if let Some(f) = &fb {
        let fb_virt = f.address() as u64;
        let fb_phys = fb_virt.saturating_sub(hhdm);
        let fb_size = f.size();
        map_phys_range(mapper, frame_allocator, fb_phys, fb_phys + fb_size as u64);
    }
    crate::drivers::graphics::init_limine(fb);
}

/// Initialize architecture-specific subsystems: GDT, IDT, syscalls, HAL,
/// and frame tracker.
#[cfg(not(target_arch = "aarch64"))]
pub fn init_architecture() {
    use crate::arch::Arch;
    crate::gdt::init();
    crate::interrupts::init_idt();
    crate::syscalls::init();
    let platform_info = crate::arch::CurrentArch::probe_platform();
    crate::hal::platform::init(platform_info);
    <crate::arch::CurrentArch as crate::arch::Arch>::init_hal_irq();
    <crate::arch::CurrentArch as crate::arch::Arch>::init_hal_timer();

    // Match the buddy allocator's usable regions; high reserved/MMIO ranges
    // must not inflate the dense refcount table beyond the kernel heap.
    let max_phys = crate::limine::iter_usable_regions()
        .map(|(_, end)| end)
        .max()
        .expect("no usable RAM for frame tracking");
    crate::memory::frame_info::init(max_phys);
    crate::memory::phys::snapshot_baseline();
}

/// Initialize platform devices: ACPI, APIC, SMP, PS/2, PCI, IOMMU, USB.
///
/// # Safety
/// Must be called after memory, heap, GDT, IDT, and HAL are initialized.
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn init_devices() {
    crate::serial_write("[BOOT] ACPI init...\n");
    crate::acpi::init(crate::limine::rsdp_addr());

    crate::serial_write("[BOOT] APIC init...\n");
    crate::apic::init();

    // Get TSC frequency from APIC calibration (now available after APIC init)
    // Keep bus frequency separate from CPU/TSC frequency
    let mut tsc_freq = crate::hal::timer::get_tsc_frequency();
    // If TSC timer not initialized yet, use estimated frequency from APIC bus calibration
    if tsc_freq == 0 {
        if let Some(est_tsc) = crate::apic::get_tsc_frequency_hz() {
            tsc_freq = est_tsc as u64;
            crate::serial_write("[BOOT] Using estimated TSC frequency from APIC bus calibration: ");
            crate::serial_write_hex(tsc_freq);
            crate::serial_write(" Hz\n");
        }
    }
    // If TSC timer not initialized yet, use estimated frequency from APIC bus calibration
    if tsc_freq == 0 {
        if let Some(est_tsc) = crate::apic::get_tsc_frequency_hz() {
            tsc_freq = est_tsc as u64;
            crate::serial_write("[BOOT] Using estimated TSC frequency from APIC bus calibration: ");
            crate::serial_write_hex(tsc_freq);
            crate::serial_write(" Hz\n");
        }
    }
    crate::serial_write("[BOOT] TSC frequency: ");
    crate::serial_write_hex(tsc_freq);
    crate::serial_write(" Hz\n");

    if tsc_freq == 0 {
        crate::serial_write(
            "[BOOT] WARNING: TSC frequency calibration failed, timing will be broken!\n",
        );
    } else if let Err(e) = crate::hal::platform::set_cpu_frequency(tsc_freq) {
        crate::serial_write("[BOOT] ERROR: Failed to set CPU frequency: ");
        crate::serial_write(e);
        crate::serial_write("\n");
    }

    // Re-initialize TSC timer now that platform info has CPU frequency
    <crate::arch::CurrentArch as crate::arch::Arch>::init_hal_timer();
    #[cfg(feature = "self_test")]
    crate::tests::run_all();
    #[cfg(feature = "smp")]
    crate::smp::init();

    crate::drivers::ps2::init();
    crate::pci::enumerate_pci();
    crate::iommu::init();
    #[cfg(feature = "iommu")]
    crate::iommu::register_vahi_arch_iommu();
    crate::drivers::usb::init();
}

/// Initialize VFS, object namespace, networking, security, and optional
/// subsystems (ASH, hypervisor).
pub fn init_vfs_network() {
    if let Some(ramdisk_data) = crate::limine::ramdisk() {
        *crate::vfs::RAMDISK.lock() = Some(ramdisk_data);
    }
    crate::vfs::init();
    crate::objects::namespace::init();
    #[cfg(feature = "net")]
    crate::net::init();

    // Enable E1000 interrupts now that the network stack is ready.
    #[cfg(all(feature = "net", not(target_arch = "aarch64")))]
    {
        if let Some(crate::drivers::net::NicDevice::E1000(ref dev)) =
            *crate::drivers::net::NIC.lock()
        {
            dev.lock().inner.enable_interrupts();
        }
    }
    crate::security::init();
    crate::sync::cfi::cfi_init();
    #[cfg(feature = "ash")]
    crate::ash::manager::init();
    #[cfg(feature = "hypervisor")]
    crate::hypervisor::init();
}
