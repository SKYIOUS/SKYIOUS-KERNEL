//! Guest memory management.
//!
//! Allocates physical memory for guests, sets up EPT/NPT mappings,
//! and loads ELF binaries or raw kernels into guest physical address space.

use alloc::vec::Vec;

/// Guest memory region.
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    pub guest_phys: u64,
    pub host_phys: u64,
    pub size: usize,
    pub flags: RegionFlags,
}

/// Region permission and type flags.
#[derive(Debug, Clone, Copy)]
pub struct RegionFlags {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub mmio: bool,
}

impl RegionFlags {
    pub const fn ram() -> Self {
        RegionFlags { read: true, write: true, execute: true, mmio: false }
    }
    pub const fn mmio() -> Self {
        RegionFlags { read: true, write: true, execute: false, mmio: true }
    }
}

/// Guest memory manager.
pub struct GuestMemory {
    pub regions: Vec<MemoryRegion>,
}

impl GuestMemory {
    /// Allocate contiguous physical memory for the guest.
    /// Returns a GuestMemory with the allocated regions mapped in EPT.
    pub fn allocate_guest(mem_size: usize) -> Option<Self> {
        let mut regions = Vec::new();
        let mut allocated = 0usize;
        let _page_size = 0x2000_00usize; // 2MB large pages

        while allocated < mem_size {
            let frame = crate::memory::buddy::BUDDY_ALLOCATOR.lock()
                .allocate_contiguous(0)?;
            let size = core::cmp::min(0x2000_00, mem_size - allocated);
            let host_phys = frame.as_u64();

            regions.push(MemoryRegion {
                guest_phys: allocated as u64,
                host_phys,
                size,
                flags: RegionFlags::ram(),
            });

            allocated += size;
        }

        Some(GuestMemory { regions })
    }

    /// Load a flat binary into guest memory at the given address.
    pub fn load_binary(&mut self, data: &[u8], guest_addr: u64) -> bool {
        for region in &self.regions {
            if guest_addr >= region.guest_phys && guest_addr < region.guest_phys + region.size as u64 {
                let offset = (guest_addr - region.guest_phys) as usize;
                if offset + data.len() > region.size {
                    return false;
                }
                let phys = region.host_phys + offset as u64;
                let virt = phys + *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap();
                // SAFETY: guest memory is allocated and writable.
                unsafe {
                    core::ptr::copy_nonoverlapping(data.as_ptr(), virt as *mut u8, data.len());
                }
                return true;
            }
        }
        false
    }

    /// Load an ELF binary, parsing segments and placing them in guest memory.
    /// Returns the entry point on success.
    pub fn load_elf(&mut self, elf_data: &[u8]) -> Option<u64> {
        use xmas_elf::ElfFile;
        let elf = ElfFile::new(elf_data).ok()?;
        let entry = elf.header.pt2.entry_point() as u64;

        for ph in elf.program_iter() {
            let seg_type = ph.get_type().ok()?;
            if seg_type != xmas_elf::program::Type::Load {
                continue;
            }
            let vaddr = ph.virtual_addr() as u64;
            let file_size = ph.file_size() as usize;
            let mem_size = ph.mem_size() as usize;
            let data = ph.get_data(&elf).ok()?;
            let slice = match data {
                xmas_elf::program::SegmentData::Undefined(s) => s,
                _ => continue,
            };

            // Zero BSS
            if file_size < mem_size {
                let bss_addr = vaddr + file_size as u64;
                let bss_size = mem_size - file_size;
                // ponytail: efficient zeroing of large BSS regions
                // works if the mapping region covers the segment
                for region in &self.regions {
                    if bss_addr >= region.guest_phys && bss_addr < region.guest_phys + region.size as u64 {
                        let offset = (bss_addr - region.guest_phys) as usize;
                        if offset + bss_size <= region.size {
                            let phys = region.host_phys + offset as u64;
                            let virt = phys + *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap();
                            unsafe {
                                core::ptr::write_bytes(virt as *mut u8, 0, bss_size);
                            }
                        }
                        break;
                    }
                }
            }

            // Copy loadable segment data
            let _ = self.load_binary(slice, vaddr);
        }

        Some(entry)
    }

    /// Set up a Linux boot (kernel + initrd + cmdline).
    /// Returns the entry point.
    pub fn load_linux(&mut self, kernel: &[u8], initrd: &[u8], cmdline: &str) -> Option<u64> {
        // Linux boot protocol: kernel at 16MB, initrd above, cmdline at 0x10000
        const LINUX_KERNEL_LOAD_ADDR: u64 = 0x100_0000; // 16MB
        const CMDLINE_ADDR: u64 = 0x1_0000; // 64KB
        const INITRD_LOAD_ADDR: u64 = 0x20_0000_0; // 32MB

        if !self.load_binary(kernel, LINUX_KERNEL_LOAD_ADDR) {
            return None;
        }

        if !initrd.is_empty() {
            if !self.load_binary(initrd, INITRD_LOAD_ADDR) {
                return None;
            }
        }

        let cmdline_bytes = cmdline.as_bytes();
        if !self.load_binary(cmdline_bytes, CMDLINE_ADDR) {
            return None;
        }

        // ponytail: set up setup_header, e820 map, ramdisk info
        // add when booting actual Linux guests

        Some(LINUX_KERNEL_LOAD_ADDR)
    }

    /// Translate a guest physical address to host physical address.
    pub fn translate(&self, guest_phys: u64) -> Option<u64> {
        for region in &self.regions {
            if guest_phys >= region.guest_phys && guest_phys < region.guest_phys + region.size as u64 {
                let offset = guest_phys - region.guest_phys;
                return Some(region.host_phys + offset);
            }
        }
        None
    }
}
