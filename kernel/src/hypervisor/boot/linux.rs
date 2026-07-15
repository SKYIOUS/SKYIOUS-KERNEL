//! Linux boot protocol (v2.12+).
//!
//! Sets up a guest for Linux kernel boot following the Linux/x86 Boot
//! Protocol. Supports both 32-bit and 64-bit (EFI stub) entry paths.

use crate::hypervisor::memory::GuestMemory;
use crate::hypervisor::boot::BootConfig;

/// Load and configure a Linux kernel for boot.
///
/// Follows the Linux/x86 boot protocol v2.12+:
/// - Kernel loaded at 16MB
/// - Setup header at offset 0x1F1 of the kernel image
/// - Command line at 0x10000
/// - Initrd loaded at a high address
/// - e820 memory map constructed
pub fn boot_linux(
    memory: &mut GuestMemory,
    kernel_data: &[u8],
    initrd: &[u8],
    cmdline: &str,
) -> Option<BootConfig> {
    const KERNEL_LOAD_ADDR: u64 = 0x100_0000;  // 16MB
    const SETUP_HDR_OFFSET: u64 = 0x1F1;
    const CMDLINE_ADDR: u64 = 0x1_0000;
    const INITRD_LOAD_ADDR: u64 = 0x20_000_00;  // 32MB
    const E820_ADDR: u64 = 0x1_4000;             // e820 map
    const E820_ENTRIES: u64 = 0x1_4E8;           // e820 entry count address

    // 1. Load kernel at 16MB
    if !memory.load_binary(kernel_data, KERNEL_LOAD_ADDR) {
        return None;
    }

    // 2. Set up setup_header fields
    let setup_sects_addr = KERNEL_LOAD_ADDR + SETUP_HDR_OFFSET + 1; // setup_sects at offset 0x1F1
    let setup_sects = if kernel_data.len() > 0 {
        ((kernel_data.len() as u64 - 0x200 + 0x1FF) / 0x200).min(127) as u8
    } else {
        0
    };

    let _ = memory.load_binary(&[setup_sects], setup_sects_addr);

    // 3. Write cmdline at 0x10000
    let cmdline_bytes = cmdline.as_bytes();
    if !memory.load_binary(cmdline_bytes, CMDLINE_ADDR) {
        return None;
    }
    let cmdline_ptr = CMDLINE_ADDR;

    // 4. Set up e820 memory map
    // ponytail: construct real e820 from boot_info.memory_regions
    // This works for a single contiguous region.
    let e820_entry_size = 20u64; // e820 entry = 20 bytes
    let num_entries = memory.regions.len().min(128) as u64;

    let region_snapshot: alloc::vec::Vec<_> = memory.regions.iter().map(|r| (r.guest_phys, r.size)).collect();
    for (i, (gphys, size)) in region_snapshot.iter().enumerate().take(128) {
        let entry_addr = E820_ADDR + (i as u64) * e820_entry_size;
        let mut entry_buf = [0u8; 20];
        entry_buf[0..8].copy_from_slice(&gphys.to_le_bytes());
        entry_buf[8..16].copy_from_slice(&(*size as u64).to_le_bytes());
        entry_buf[16..20].copy_from_slice(&1u32.to_le_bytes()); // Type 1 = usable RAM
        memory.load_binary(&entry_buf, entry_addr);
    }
    memory.load_binary(&num_entries.to_le_bytes(), E820_ENTRIES);

    // 5. Load initrd
    let _ramdisk_image = if !initrd.is_empty() {
        if !memory.load_binary(initrd, INITRD_LOAD_ADDR) {
            return None;
        }

        // Write ramdisk info into setup header
        let ramdisk_image_addr = KERNEL_LOAD_ADDR + 0x218u64; // hdr.ramdisk_image
        let ramdisk_size_addr = KERNEL_LOAD_ADDR + 0x21Cu64;  // hdr.ramdisk_size
        memory.load_binary(&INITRD_LOAD_ADDR.to_le_bytes(), ramdisk_image_addr);
        memory.load_binary(&(initrd.len() as u64).to_le_bytes(), ramdisk_size_addr);
        INITRD_LOAD_ADDR
    } else {
        0
    };

    // 6. Write cmdline pointer into setup header
    let cmdline_ptr_addr = KERNEL_LOAD_ADDR + 0x220u64; // hdr.cmd_line_ptr
    memory.load_binary(&cmdline_ptr.to_le_bytes(), cmdline_ptr_addr);

    // 7. Set loadflags (0x211) — bit 1 = load_high, bit 7 = keep_segments
    let loadflags_addr = KERNEL_LOAD_ADDR + 0x211u64;
    memory.load_binary(&[0xA1u8], loadflags_addr); // CAN_USE_HEAP + LOADED_HIGH + KEEP_SEGMENTS

    // 8. Set heap end pointer (hdr.heap_end_ptr at 0x224)
    let heap_end_ptr_addr = KERNEL_LOAD_ADDR + 0x224u64;
    memory.load_binary(&0x8000u16.to_le_bytes(), heap_end_ptr_addr);

    // Determine entry point: 32-bit boot protocol uses KERNEL_LOAD_ADDR + 0x200
    let entry_point = KERNEL_LOAD_ADDR + 0x200;

    // 9. Set up boot params signature
    // hdr.boot_flag at offset 0x1FE = 0xAA55
    let boot_flag_addr = KERNEL_LOAD_ADDR + 0x1FE;
    memory.load_binary(&0x55AAu16.to_le_bytes(), boot_flag_addr);

    Some(BootConfig {
        entry_point,
        boot_data: alloc::vec![0u8; 0],
        cpu_count: 1,
    })
}
