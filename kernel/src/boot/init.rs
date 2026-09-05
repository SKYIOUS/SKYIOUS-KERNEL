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
    crate::serial_write("[BOOT] memory::init...\n");
    let mut mapper = unsafe { crate::memory::init(phys_mem_offset) };
    crate::serial_write("[BOOT] memory::init done\n");

    crate::serial_write("[BOOT] frame allocator...\n");
    unsafe { crate::memory::init_frame_allocator_limine() };
    let mut frame_allocator = BuddyFrameAllocator;
    crate::serial_write("[BOOT] heap init...\n");
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
    crate::serial_write("[BOOT] HHDM mapping done\n");

    (mapper, frame_allocator)
}

/// Map framebuffer pages and initialize graphics subsystem.
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
        crate::serial_write("[BOOT] fb pages mapped\n");
        crate::serial_write("[BOOT] fb=present\n");
    } else {
        crate::serial_write("[BOOT] fb=NONE\n");
    }
    crate::serial_write("[BOOT] graphics init...\n");
    crate::drivers::graphics::init_limine(fb);
    crate::serial_write("[BOOT] graphics init done\n");

    if crate::drivers::graphics::is_active() {
        crate::serial_write("[BOOT] graphics=active\n");
        // Show boot splash as soon as framebuffer is ready.
        crate::gui::splash::init();
    } else {
        crate::serial_write("[BOOT] graphics=INACTIVE\n");
    }
    crate::serial_write("[BOOT] -> SARGA OS \u{2014} Vahi Kernel v0.3.0 starting...\n");
    crate::serial_write("[SPLASH] SARGA OS loading...\n");
    // ponytail: 1M spin_loop iterations took ~90s in debug+TCG; 10k is
    // enough for a human to read a framebuffer splash.
    for _ in 0..10_000 {
        core::hint::spin_loop();
    }
}

/// Initialize architecture-specific subsystems: GDT, IDT, syscalls, HAL,
/// and frame tracker.
#[cfg(not(target_arch = "aarch64"))]
pub fn init_architecture() {
    use crate::arch::Arch;
    crate::serial_write("[BOOT] gdt init...\n");
    crate::gdt::init();
    crate::serial_write("[BOOT] idt+pic init...\n");
    crate::interrupts::init_idt();
    crate::serial_write("[BOOT] syscalls init...\n");
    crate::syscalls::init();

    crate::serial_write("[BOOT] HAL init...\n");
    let platform_info = crate::arch::CurrentArch::probe_platform();
    crate::hal::platform::init(platform_info);
    crate::arch::CurrentArch::init_hal_irq();
    crate::arch::CurrentArch::init_hal_timer();
    crate::serial_write("[BOOT] HAL init done\n");

    crate::serial_write("[BOOT] frame tracker init...\n");
    let max_phys = crate::limine::max_physical_address();
    crate::memory::frame_info::init(max_phys);
    crate::memory::phys::snapshot_baseline();
    crate::serial_write("[BOOT] -> VAHI Frame Tracker: OK\n");
}

/// Initialize platform devices: ACPI, APIC, SMP, PS/2, PCI, IOMMU, USB.
///
/// # Safety
/// Must be called after memory, heap, GDT, IDT, and HAL are initialized.
#[cfg(not(target_arch = "aarch64"))]
pub fn init_devices() {
    crate::serial_write("[BOOT] ACPI init...\n");
    crate::acpi::init(crate::limine::rsdp_addr());
    crate::serial_write("[BOOT] APIC init...\n");
    crate::apic::init();
    crate::tests::run_all();
    #[cfg(feature = "smp")]
    {
        crate::serial_write("[BOOT] SMP init...\n");
        crate::smp::init();
    }
    crate::serial_write("[BOOT] PS/2 init...\n");
    crate::drivers::ps2::init();
    crate::serial_write("[BOOT] PCI enumerate...\n");
    crate::pci::enumerate_pci();
    crate::serial_write("[BOOT] IOMMU init...\n");
    crate::iommu::init();
    crate::serial_write("[BOOT] USB init...\n");
    crate::drivers::usb::init();
}

/// Initialize VFS, object namespace, networking, security, and optional
/// subsystems (ASH, hypervisor).
pub fn init_vfs_network() {
    crate::serial_write("[BOOT] VFS init...\n");
    if let Some(ramdisk_data) = crate::limine::ramdisk() {
        *crate::vfs::RAMDISK.lock() = Some(ramdisk_data);
        crate::serial_write("[BOOT] initrd from Limine modules\n");
    }
    crate::vfs::init();
    crate::serial_write("[BOOT] object manager init...\n");
    crate::objects::namespace::init();
    #[cfg(feature = "net")]
    {
        crate::serial_write("[BOOT] net init...\n");
        crate::net::init();
    }

    // Enable E1000 interrupts now that the network stack is ready.
    #[cfg(all(feature = "net", not(target_arch = "aarch64")))]
    {
        if let Some(crate::drivers::net::NicDevice::E1000(ref dev)) =
            *crate::drivers::net::NIC.lock()
        {
            dev.lock().inner.enable_interrupts();
        }
    }
    crate::serial_write("[BOOT] LSM init...\n");
    crate::security::init();
    crate::serial_write("[BOOT] CFI init...\n");
    crate::sync::cfi::cfi_init();
    #[cfg(feature = "ash")]
    {
        crate::serial_write("[BOOT] ASH init...\n");
        crate::ash::manager::init();
    }
    #[cfg(feature = "hypervisor")]
    {
        crate::serial_write("[BOOT] hypervisor init...\n");
        crate::hypervisor::init();
    }
    crate::serial_write("[BOOT] -> SARGA OS: Graphical Console Mode Active!\n");
}
