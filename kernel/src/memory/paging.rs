//! # Virtual Memory Paging
//!
//! This module provides high-level abstractions for managing virtual address spaces
//! and page tables.

use x86_64::{
    structures::paging::{Page, PageTable, OffsetPageTable, FrameAllocator, PhysFrame, Size4KiB},
    registers::control::Cr3,
    VirtAddr,
};
use crate::memory;

/// Represents a virtual address space (a process's page table).
pub struct AddressSpace {
    /// The physical address of the Level 4 Page Table.
    pml4_frame: PhysFrame,
}

impl AddressSpace {
    /// Creates a new address space by cloning the kernel's higher-half mappings.
    pub fn new(frame_allocator: &mut impl FrameAllocator<Size4KiB>) -> Option<Self> {
        let pml4_frame = frame_allocator.allocate_frame()?;
        
        let phys_offset = *memory::PHYSICAL_MEMORY_OFFSET.get()?;
        let virt_offset = VirtAddr::new(phys_offset);

        // Get the current (kernel) PML4
        let (current_pml4_frame, _) = Cr3::read();
        let current_pml4_virt = virt_offset + current_pml4_frame.start_address().as_u64();
        let current_pml4 = unsafe { &*(current_pml4_virt.as_ptr() as *const PageTable) };

        // Get the new PML4
        let new_pml4_virt = virt_offset + pml4_frame.start_address().as_u64();
        let new_pml4 = unsafe { &mut *(new_pml4_virt.as_mut_ptr() as *mut PageTable) };

        // Zero the new PML4
        new_pml4.zero();

        // Copy kernel mappings (entries 256..512 for higher-half kernels)
        // Vahi uses 0xFFFF_8000_0000_0000 and above for kernel.
        // Index 256 starts at 0xFFFF_8000_0000_0000.
        for i in 256..512 {
            new_pml4[i] = current_pml4[i].clone();
        }

        Some(AddressSpace { pml4_frame })
    }

    /// Activates this address space by switching CR3.
    pub unsafe fn activate(&self) {
        let (_, flags) = Cr3::read();
        Cr3::write(self.pml4_frame, flags);
    }

    /// Returns the physical address of the PML4.
        pub fn _pml4_phys(&self) -> PhysFrame {
        self.pml4_frame
    }
    
    /// Provides a mapper for this address space.
    pub unsafe fn mapper(&self) -> Option<OffsetPageTable<'static>> {
        let phys_offset = *memory::PHYSICAL_MEMORY_OFFSET.get()?;
        let virt_offset = VirtAddr::new(phys_offset);
        let pml4_virt = virt_offset + self.pml4_frame.start_address().as_u64();
        let pml4 = &mut *(pml4_virt.as_mut_ptr() as *mut PageTable);
        Some(OffsetPageTable::new(pml4, virt_offset))
    }

    /// Clones this address space using Copy-on-Write for user-space pages.
    pub fn clone_cow(&self, frame_allocator: &mut impl FrameAllocator<Size4KiB>) -> Option<Self> {
        let new_pml4_frame = frame_allocator.allocate_frame()?;
        let phys_offset = *memory::PHYSICAL_MEMORY_OFFSET.get()?;
        let virt_offset = VirtAddr::new(phys_offset);

        let old_pml4_virt = virt_offset + self.pml4_frame.start_address().as_u64();
        let old_pml4 = unsafe { &*(old_pml4_virt.as_ptr() as *const PageTable) };

        let new_pml4_virt = virt_offset + new_pml4_frame.start_address().as_u64();
        let new_pml4 = unsafe { &mut *(new_pml4_virt.as_mut_ptr() as *mut PageTable) };
        new_pml4.zero();

        // 1. Copy kernel entries (Direct Move/Shared)
        for i in 256..512 {
            new_pml4[i] = old_pml4[i].clone();
        }

        // 2. Clone user entries with COW
        for i in 0..256 {
            if !old_pml4[i].is_unused() {
                // We need to deep clone the page table hierarchy for user space
                Self::clone_recursive(i, old_pml4, new_pml4, 4, virt_offset, frame_allocator)?;
            }
        }

        Some(AddressSpace { pml4_frame: new_pml4_frame })
    }

    fn clone_recursive(
        index: usize,
        src_table: &PageTable,
        dst_table: &mut PageTable,
        level: u8,
        virt_offset: VirtAddr,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>
    ) -> Option<()> {
        use x86_64::structures::paging::PageTableFlags;

        let entry = &src_table[index];
        if entry.is_unused() {
            return Some(());
        }

        if level == 1 {
            // This is a leaf page entry
            let mut flags = entry.flags();
            
            // If it's writable, we make it read-only and mark it COW
            if flags.contains(PageTableFlags::WRITABLE) {
                flags.remove(PageTableFlags::WRITABLE);
                // Bit 9 is available for software. We use it for COW.
                let mut bits = flags.bits();
                bits |= 1 << 9; 
                flags = PageTableFlags::from_bits_truncate(bits);
            }

            dst_table[index].set_addr(entry.addr(), flags);
            
            // Increment refcount for the physical frame
            memory::frame_info::increment(entry.addr());

            return Some(());
        }

        // Otherwise, it's a pointer to a lower level table.
        // Allocate a new table for the child.
        let new_frame = frame_allocator.allocate_frame()?;
        dst_table[index].set_frame(new_frame, entry.flags());

        let next_src_virt = virt_offset + entry.addr().as_u64();
        let next_src_table = unsafe { &*(next_src_virt.as_ptr() as *const PageTable) };
        
        let next_dst_virt = virt_offset + new_frame.start_address().as_u64();
        let next_dst_table = unsafe { &mut *(next_dst_virt.as_mut_ptr() as *mut PageTable) };
        next_dst_table.zero();

        for i in 0..512 {
            Self::clone_recursive(i, next_src_table, next_dst_table, level - 1, virt_offset, frame_allocator)?;
        }

        Some(())
    }

    /// Destroy this address space: free all user pages and page table frames.
    /// Does NOT free kernel pages (entries 256..512) since they belong to all processes.
    pub fn destroy(&self) {
        use x86_64::structures::paging::PageTableFlags;
        let phys_offset = *memory::PHYSICAL_MEMORY_OFFSET.get()
            .expect("PHYSICAL_MEMORY_OFFSET not initialized");
        let virt_offset = VirtAddr::new(phys_offset);

        let pml4_virt = virt_offset + self.pml4_frame.start_address().as_u64();
        let pml4 = unsafe { &*(pml4_virt.as_ptr() as *const PageTable) };

        // Walk user entries (0..256)
        for p4_idx in 0..256 {
            if !pml4[p4_idx].is_unused() {
                let flags = pml4[p4_idx].flags();
                if flags.contains(PageTableFlags::HUGE_PAGE) {
                    // 1 GiB huge page — free the frame
                    if let Ok(frame) = pml4[p4_idx].frame() {
                        memory::frame_info::decrement(frame.start_address());
                    }
                    continue;
                }
                if let Ok(p3_frame) = pml4[p4_idx].frame() {
                    let p3_virt = virt_offset + p3_frame.start_address().as_u64();
                    let p3_table = unsafe { &*(p3_virt.as_ptr() as *const PageTable) };
                    Self::destroy_table(p3_table, 3, virt_offset, p3_frame);
                }
            }
        }

        // Free the PML4 frame itself
        memory::frame_info::decrement(self.pml4_frame.start_address());
        crate::memory::buddy::BUDDY_ALLOCATOR.lock()
            .deallocate_frame(self.pml4_frame);
    }

    fn destroy_table(table: &PageTable, level: u8, virt_offset: VirtAddr, own_frame: PhysFrame) {
        use x86_64::structures::paging::PageTableFlags;
        for i in 0..512 {
            if table[i].is_unused() { continue; }
            let flags = table[i].flags();

            if level == 1 || flags.contains(PageTableFlags::HUGE_PAGE) {
                // Leaf page: decrement refcount
                if let Ok(frame) = table[i].frame() {
                    memory::frame_info::decrement(frame.start_address());
                }
            } else {
                // Non-leaf: recurse into child table
                if let Ok(child_frame) = table[i].frame() {
                    let child_virt = virt_offset + child_frame.start_address().as_u64();
                    let child_table = unsafe { &*(child_virt.as_ptr() as *const PageTable) };
                    Self::destroy_table(child_table, level - 1, virt_offset, child_frame);
                }
            }
        }

        // Free the page table frame itself (but NOT the PML4 — that's freed by destroy())
        crate::memory::buddy::BUDDY_ALLOCATOR.lock()
            .deallocate_frame(own_frame);
        memory::frame_info::decrement(own_frame.start_address());
    }

    /// Handles a Copy-on-Write fault for a given page.
    /// Returns Some(true) if the fault was resolved, Some(false) if it was a COW page but resolution failed,
    /// and None if it was not a COW page.
    pub unsafe fn handle_cow(&self, page: Page<Size4KiB>) -> Option<bool> {
        use x86_64::structures::paging::{PageTableFlags, FrameAllocator};
        use crate::memory::buddy::BuddyFrameAllocator;

        let phys_offset = *memory::PHYSICAL_MEMORY_OFFSET.get()?;
        let virt_offset = VirtAddr::new(phys_offset);
        let pml4_virt = virt_offset + self.pml4_frame.start_address().as_u64();
        let pml4 = &mut *(pml4_virt.as_mut_ptr() as *mut PageTable);
        
        // Manual walk to get the entry
        let p4_idx = page.p4_index();
        let p3_idx = page.p3_index();
        let p2_idx = page.p2_index();
        let p1_idx = page.p1_index();

        let p3_table_frame = pml4[p4_idx].frame().ok()?;
        let p3_table = &mut *( (virt_offset + p3_table_frame.start_address().as_u64()).as_mut_ptr() as *mut PageTable );
        
        let p2_table_frame = p3_table[p3_idx].frame().ok()?;
        let p2_table = &mut *( (virt_offset + p2_table_frame.start_address().as_u64()).as_mut_ptr() as *mut PageTable );
        
        let p1_table_frame = p2_table[p2_idx].frame().ok()?;
        let p1_table = &mut *( (virt_offset + p1_table_frame.start_address().as_u64()).as_mut_ptr() as *mut PageTable );
        
        let entry = &mut p1_table[p1_idx];
        let flags = entry.flags();
        
        // Bit 9 is COW
        if (flags.bits() & (1 << 9)) == 0 {
            return None; // Not a COW page
        }

        let old_frame = entry.frame().ok()?;
        let ref_count = memory::frame_info::count(old_frame.start_address());

        if ref_count > 1 {
            // Allocate new frame and copy
            let mut frame_allocator = BuddyFrameAllocator;
            let new_frame = frame_allocator.allocate_frame()?;
            
            // Copy data
            let old_ptr = (virt_offset + old_frame.start_address().as_u64()).as_ptr::<u8>();
            let new_ptr = (virt_offset + new_frame.start_address().as_u64()).as_mut_ptr::<u8>();
            core::ptr::copy_nonoverlapping(old_ptr, new_ptr, 4096);

            // Update page table
            let mut new_flags = flags;
            new_flags.insert(PageTableFlags::WRITABLE);
            let mut bits = new_flags.bits();
            bits &= !(1 << 9); // Clear COW bit
            new_flags = PageTableFlags::from_bits_truncate(bits);
            
            entry.set_frame(new_frame, new_flags);
            
            // Refcount management
            memory::frame_info::decrement(old_frame.start_address());
        } else {
            // Only one reference remains, just make it writable
            let mut new_flags = flags;
            new_flags.insert(PageTableFlags::WRITABLE);
            let mut bits = new_flags.bits();
            bits &= !(1 << 9); // Clear COW bit
            new_flags = PageTableFlags::from_bits_truncate(bits);
            
            entry.set_flags(new_flags);
        }

        use x86_64::instructions::tlb;
        tlb::flush(page.start_address());

        // Phase H1: TLB shootdown via IPI
        #[cfg(feature = "smp")]
        crate::smp::broadcast_tlb_flush(page.start_address().as_u64());

        Some(true)
    }
}
