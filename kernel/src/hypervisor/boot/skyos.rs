//! SkyOS guest boot protocol.
//!
//! Boots a SkyOS instance as a guest VM. The SkyOS guest receives a
//! BootInfo structure (similar to the native boot) describing its
//! memory map, framebuffer, and hypervisor interface.

use crate::hypervisor::memory::GuestMemory;
use crate::hypervisor::boot::BootConfig;

/// Boot a SkyOS guest.
///
/// SkyOS guests expect:
/// - Kernel loaded at a configurable address (default 2MB)
/// - BootInfo structure at a known address passed via register
/// - Initial page tables set up by the hypervisor
/// - CPU in long mode with paging enabled
pub fn boot_skyos(
    memory: &mut GuestMemory,
    kernel_data: &[u8],
    mem_size: usize,
) -> Option<BootConfig> {
    const BOOTINFO_ADDR: u64 = 0x1_000;        // 4KB

    // 1. Load kernel ELF
    let entry = memory.load_elf(kernel_data)?;

    // 2. Allocate boot info structure
    // For now, allocate a simple BootInfo with memory map entries
    let mut bootinfo_data = alloc::vec![0u8; 256];

    // Number of memory map entries at offset 0
    let num_entries = memory.regions.len().min(16) as u64;
    bootinfo_data[0..8].copy_from_slice(&num_entries.to_le_bytes());

    // Memory map entries starting at offset 8
    // Each entry: base(8) + size(8) + type(8) = 24 bytes
    for (i, region) in memory.regions.iter().enumerate().take(16) {
        let off = 8 + i * 24;
        if off + 24 > bootinfo_data.len() {
            break;
        }
        bootinfo_data[off..off + 8].copy_from_slice(&region.guest_phys.to_le_bytes());
        bootinfo_data[off + 8..off + 16].copy_from_slice(&(region.size as u64).to_le_bytes());
        bootinfo_data[off + 16..off + 20].copy_from_slice(&1u32.to_le_bytes()); // Type = RAM
    }

    // Total memory size at offset 200
    bootinfo_data[200..208].copy_from_slice(&(mem_size as u64).to_le_bytes());

    // 3. Write bootinfo to guest memory
    if !memory.load_binary(&bootinfo_data, BOOTINFO_ADDR) {
        return None;
    }

    // 4. Set up initial page tables for the SkyOS kernel
    // ponytail: identity-map the kernel region so the guest can start paging
    // add when EPT manager is wired to the boot path

    Some(BootConfig {
        entry_point: entry,
        boot_data: bootinfo_data,
        cpu_count: 1,
    })
}
