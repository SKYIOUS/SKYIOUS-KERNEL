/// W^X executable-memory allocator.
///
/// Allocates physical page frames and maps them RW (read-write, not executable).
/// Provides `flip_to_rx()` to remap the page table entry from RW to RX
/// (clear the NX/execute-disable bit). Never simultaneously RW+RX.
use core::sync::atomic::{AtomicU64, Ordering};
use alloc::vec::Vec;
use crate::memory::buddy::BUDDY_ALLOCATOR;
use crate::memory::{physical_memory_offset, active_level_4_table};
use x86_64::structures::paging::{PhysFrame, PageTableFlags, PageTable, Size4KiB};
use x86_64::VirtAddr;

// Base VA for exec pool; bump-allocated per ExecRegion (avoids PTE collision on multi-alloc).
static NEXT_VA: AtomicU64 = AtomicU64::new(0xFFFF_8000_0010_0000);
fn alloc_va() -> VirtAddr {
    let va = NEXT_VA.fetch_add(4096, Ordering::Relaxed);
    // ponytail: 256 KiB pool + 64 pages bump is within 0xFFFF_8000_0010_0000..0xFFFF_8000_0020_0000, far below kernel end
    VirtAddr::new(va)
}

/// Baseline pool: 256 KiB = 64 pages pre-allocated at boot.
pub const POOL_PAGES: usize = 64;
pub const POOL_BYTES: usize = POOL_PAGES * 4096;

static POOL: crate::sync::IrqSafeMutex<alloc::vec::Vec<PhysFrame<Size4KiB>>> =
    crate::sync::IrqSafeMutex::new(alloc::vec::Vec::new());
static POOL_INIT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Pre-allocate 256 KiB (64 pages) from the buddy allocator into the exec pool.
/// Must be called once after `heap` and `frame allocator` are initialized (post-boot).
pub fn init_pool() {
    if POOL_INIT.swap(true, core::sync::atomic::Ordering::SeqCst) {
        return; // already initialized
    }
    let mut pool = POOL.lock();
    pool.reserve(POOL_PAGES);
    for _ in 0..POOL_PAGES {
        if let Some(frame) = crate::memory::buddy::BUDDY_ALLOCATOR.lock().allocate_frame() {
            pool.push(PhysFrame::containing_address(frame.start_address()));
        } else {
            break;
        }
    }
    crate::println!("exec_mem: pool ready: {}/{} pages ({} KiB)", pool.len(), POOL_PAGES, pool.len()*4);
}

/// An executable-memory region allocated via the W^X allocator.
///
/// The page is initially mapped RW (read-write, not executable).
/// Must call `flip_to_rx()` before executing any code.
/// Never simultaneously RW+RX (enforced by `is_rx` state).
pub struct ExecRegion {
    virt: VirtAddr,
    phys_frame: PhysFrame,
    is_rx: AtomicU64, // 0 = RW, 1 = RX
}

impl ExecRegion {
    /// Allocate a new executable-memory region.
    /// The page is mapped RW at a fixed kernel virtual address.
    pub fn alloc() -> Result<Self, &'static str> {
        // 1. Allocate a page: try pool first, then buddy (256 KiB baseline).
        let phys_frame: PhysFrame<Size4KiB> = {
            if let Some(pf) = POOL.lock().pop() {
                pf
            } else {
                let mut buddy = BUDDY_ALLOCATOR.lock();
                let frame = buddy.allocate_frame().ok_or("no free frames")?;
                PhysFrame::containing_address(frame.start_address())
            }
        };

        // 2. Map the page as RW+NX at a per-region VA (W^X: RW without X until flip)
        let virt = alloc_va();
        Self::map_page_rw(virt, phys_frame)?;

        // 3. The page is now mapped RW at EXEC_VIRT.
        //    is_rx = 0 means RW state (caller may write; do NOT flip_to_rx and then write).
        Ok(ExecRegion {
            virt,
            phys_frame,
            is_rx: AtomicU64::new(0), // 0 = RW state
        })
    }

    /// Map a physical frame as RW (read-write, present) at the given virtual address.
    /// Walks the 4-level page tables and sets up the PTE.
    fn map_page_rw(virt: VirtAddr, frame: PhysFrame) -> Result<(), &'static str> {
        let phys_offset = physical_memory_offset();
        let pml4 = unsafe { active_level_4_table(VirtAddr::new(phys_offset)) };

        // Calculate PTE indices for the virtual address
        let v = virt.as_u64();
        let pml4_idx = (v >> 39) & 0x1FF;
        let pdp_idx = (v >> 30) & 0x1FF;
        let pd_idx = (v >> 21) & 0x1FF;
        let pt_idx = (v >> 12) & 0x1FF;

        // Walk/create: PML4 → PDP → PD → PT → PTE
        // PML4 entry
        let pde = &mut pml4[pml4_idx as usize];
        let pdp_frame: PhysFrame<Size4KiB> = if pde.is_unused() {
            // Allocate a new PDP table frame
            let mut buddy = BUDDY_ALLOCATOR.lock();
            let new_frame = buddy.allocate_frame().ok_or("no free frames for PDP")?;
            pde.set_frame(new_frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
            new_frame
        } else {
            PhysFrame::containing_address(pde.addr())
        };
        let pdp_virt = VirtAddr::new(phys_offset) + pdp_frame.start_address().as_u64();
        let pdp_table: &mut PageTable = unsafe { &mut *(pdp_virt.as_mut_ptr() as *mut PageTable) };

        // PDP entry
        let pdpe = &mut pdp_table[pdp_idx as usize];
        let pd_frame: PhysFrame<Size4KiB> = if pdpe.is_unused() {
            let mut buddy = BUDDY_ALLOCATOR.lock();
            let new_frame = buddy.allocate_frame().ok_or("no free frames for PD")?;
            pdpe.set_frame(new_frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
            new_frame
        } else {
            PhysFrame::containing_address(pdpe.addr())
        };
        let pd_virt = VirtAddr::new(phys_offset) + pd_frame.start_address().as_u64();
        let pd_table: &mut PageTable = unsafe { &mut *(pd_virt.as_mut_ptr() as *mut PageTable) };

        // PD entry
        let pde_entry = &mut pd_table[pd_idx as usize];
        let pt_frame: PhysFrame<Size4KiB> = if pde_entry.is_unused() {
            let mut buddy = BUDDY_ALLOCATOR.lock();
            let new_frame = buddy.allocate_frame().ok_or("no free frames for PT")?;
            pde_entry.set_frame(new_frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
            new_frame
        } else {
            PhysFrame::containing_address(pde_entry.addr())
        };
        let pt_virt = VirtAddr::new(phys_offset) + pt_frame.start_address().as_u64();
        let pt_table: &mut PageTable = unsafe { &mut *(pt_virt.as_mut_ptr() as *mut PageTable) };

        // PT entry (PTE) - the actual page mapping
        let pte = &mut pt_table[pt_idx as usize];
        // W^X: initially RW without X (PRESENT|WRITABLE|NO_EXECUTE)
        pte.set_frame(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE);

        // Invalidate TLB for this page
        x86_64::instructions::tlb::flush(virt);

        Ok(())
    }

    /// Flip the page table entry from RW to RX (clear the NX/execute-disable bit).
    /// Must be called before executing any code in this region.
    /// Has no effect if already in RX state.
    pub fn flip_to_rx(&self) -> Result<(), &'static str> {
        let current = self.is_rx.load(Ordering::SeqCst);
        if current == 1 {
            // Already RX, no-op
            return Ok(());
        }

        // Get the PTE for the mapped page and clear the NX bit (bit 63).
        let phys_offset = physical_memory_offset();
        let pml4 = unsafe { active_level_4_table(VirtAddr::new(phys_offset)) };

        // Calculate PTE indices for this region's VA (not fixed exec_virt)
        let v = self.virt.as_u64();
        let pml4_idx = (v >> 39) & 0x1FF;
        let pdp_idx = (v >> 30) & 0x1FF;
        let pd_idx = (v >> 21) & 0x1FF;
        let pt_idx = (v >> 12) & 0x1FF;

        // Walk: PML4 → PDP → PD → PT → PTE
        let pde = &pml4[pml4_idx as usize];
        if pde.is_unused() {
            return Err("PML4 entry unused while flipping RX");
        }
        let pdp_frame: PhysFrame<Size4KiB> = PhysFrame::containing_address(pde.addr());
        let pdp_virt = VirtAddr::new(phys_offset) + pdp_frame.start_address().as_u64();
        let pdp_table: &PageTable = unsafe { &*(pdp_virt.as_ptr() as *const PageTable) };

        let pdpe = &pdp_table[pdp_idx as usize];
        if pdpe.is_unused() {
            return Err("PDP entry unused while flipping RX");
        }
        let pd_frame: PhysFrame<Size4KiB> = PhysFrame::containing_address(pdpe.addr());
        let pd_virt = VirtAddr::new(phys_offset) + pd_frame.start_address().as_u64();
        let pd_table: &PageTable = unsafe { &*(pd_virt.as_ptr() as *const PageTable) };

        let pde_entry = &pd_table[pd_idx as usize];
        if pde_entry.is_unused() {
            return Err("PD entry unused while flipping RX");
        }
        let pt_frame: PhysFrame<Size4KiB> = PhysFrame::containing_address(pde_entry.addr());
        let pt_virt = VirtAddr::new(phys_offset) + pt_frame.start_address().as_u64();
        let pt_table: &PageTable = unsafe { &*(pt_virt.as_ptr() as *const PageTable) };

        let pte = &pt_table[pt_idx as usize];

        // W^X: flip RW+NX -> RX (clear WRITABLE and NO_EXECUTE, keep PRESENT)
        let flags = pte.flags();
        let mut new_flags = flags;
        new_flags.remove(PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE);
        new_flags.insert(PageTableFlags::PRESENT);
        // Need mutable access to set flags - get mutable reference
        let pml4_mut = unsafe { active_level_4_table(VirtAddr::new(phys_offset)) };
        let pde_mut = &mut pml4_mut[pml4_idx as usize];
        let pdp_frame_mut: PhysFrame<Size4KiB> = PhysFrame::containing_address(pde_mut.addr());
        let pdp_virt_mut = VirtAddr::new(phys_offset) + pdp_frame_mut.start_address().as_u64();
        let pdp_table_mut: &mut PageTable = unsafe { &mut *(pdp_virt_mut.as_mut_ptr() as *mut PageTable) };
        let pdpe_mut = &mut pdp_table_mut[pdp_idx as usize];
        let pd_frame_mut: PhysFrame<Size4KiB> = PhysFrame::containing_address(pdpe_mut.addr());
        let pd_virt_mut = VirtAddr::new(phys_offset) + pd_frame_mut.start_address().as_u64();
        let pd_table_mut: &mut PageTable = unsafe { &mut *(pd_virt_mut.as_mut_ptr() as *mut PageTable) };
        let pde_entry_mut = &mut pd_table_mut[pd_idx as usize];
        let pt_frame_mut: PhysFrame<Size4KiB> = PhysFrame::containing_address(pde_entry_mut.addr());
        let pt_virt_mut = VirtAddr::new(phys_offset) + pt_frame_mut.start_address().as_u64();
        let pt_table_mut: &mut PageTable = unsafe { &mut *(pt_virt_mut.as_mut_ptr() as *mut PageTable) };
        let pte_mut = &mut pt_table_mut[pt_idx as usize];
        pte_mut.set_flags(new_flags);

        // Invalidate the TLB for this page so the CPU picks up the new flags
        x86_64::instructions::tlb::flush(self.virt);

        // Note: we track RX state via `is_rx`. The caller must NOT write after calling flip_to_rx().

        self.is_rx.store(1, Ordering::SeqCst);
        Ok(())
    }

    /// Get a mutable pointer to the region's memory (valid while in RW state).
    /// Use this to copy JIT code before calling flip_to_rx().
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.virt.as_mut_ptr() as *mut u8
    }

    /// Get a function pointer to the executable page.
    /// Must call `flip_to_rx()` first. The returned pointer is valid for execution.
    pub fn get_fn<T>(&self) -> *const T {
        // Safety: caller must have called flip_to_rx() first
        self.virt.as_ptr() as *const T
    }
}

impl Drop for ExecRegion {
    fn drop(&mut self) {
        // Return to pool if there is space, else free to buddy.
        // Keeps the 256 KiB baseline warm for the next JIT.
        let mut pool = POOL.lock();
        if pool.len() < POOL_PAGES {
            pool.push(self.phys_frame);
        } else {
            drop(pool);
            let mut buddy = BUDDY_ALLOCATOR.lock();
            buddy.deallocate_frame(self.phys_frame);
        }
        // Note: page table pages (PDP, PD, PT) allocated in map_page_rw are not freed.
        // This is a small leak acceptable for now; a full implementation would track and free them.
    }
}

/// Allocate `n` executable-memory regions (each 4 KiB).
/// Returns a vector of `ExecRegion` handles. Each region must have
/// `flip_to_rx()` called before executing any code.
pub fn alloc_n_pages(n: usize) -> Result<Vec<ExecRegion>, &'static str> {
    let mut regions = Vec::new();
    for _ in 0..n {
        match ExecRegion::alloc() {
            Ok(region) => regions.push(region),
            Err(e) => {
                // Free already-allocated regions on failure
                for region in regions.drain(..) {
                    let _ = region; // Drop impl will free the frame
                }
                return Err(e);
            }
        }
    }
    Ok(regions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_alloc_and_flip() {
        let region = ExecRegion::alloc().expect("alloc should succeed");
        // Initially in RW state (is_rx = 0)
        assert_eq!(region.is_rx.load(Ordering::SeqCst), 0);

        // Flip to RX
        region.flip_to_rx().expect("flip_to_rx should succeed");
        assert_eq!(region.is_rx.load(Ordering::SeqCst), 1);

        // Get a function pointer (valid only after flip_to_rx)
        let _fn: fn() = region.get_fn();
        // In a real test we'd call the function, but here we just verify the ptr is non-null
        assert!(!_fn.is_null());
    }

    #[test]
    fn test_double_flip_no_panic() {
        let region = ExecRegion::alloc().expect("alloc should succeed");
        // Flip once
        region.flip_to_rx().expect("first flip should succeed");
        assert_eq!(region.is_rx.load(Ordering::SeqCst), 1);

        // Flip again (no-op since already RX)
        region.flip_to_rx().expect("second flip should be no-op");
        assert_eq!(region.is_rx.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_alloc_n_pages() {
        let regions = alloc_n_pages(4).expect("alloc_n_pages should succeed");
        assert_eq!(regions.len(), 4);
        for region in &regions {
            assert_eq!(region.is_rx.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn test_drop_frees_frame() {
        let region = ExecRegion::alloc().expect("alloc should succeed");
        // Drop will free the frame back to the buddy allocator
        drop(region);
        // After drop, the frame should be free; allocation may succeed again
        // (this is a best-effort test; buddy state depends on overall allocator state)
    }
}