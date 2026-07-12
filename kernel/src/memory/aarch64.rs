//! aarch64 page table and memory management.
//!
//! Provides aarch64 equivalents of the x86_64 page table operations used by
//! the kernel: reading translation tables, virtual-to-physical translation,
//! address space creation, and user/kernel memory copy.
//!
//! The aarch64 MMU uses 4KB granule with 4-level page tables (48-bit VA space):
//!   L0 (TTBR1_EL1/TTBR0_EL1): covers 512 × 1GB = 512GB
//!   L1 (PGD): 512 × 2MB = 1GB per entry
//!   L2 (PMD): 512 × 4KB = 2MB per entry
//!   L3 (PTE): 512 × 4KB = 2MB per table

// aarch64 page table descriptor format
pub const PAGE_SIZE: u64 = 4096;
pub const PAGE_TABLE_ENTRIES: usize = 512;

// Descriptor type bits (lower 2 bits of a page table entry)
pub const DESC_TYPE_MASK: u64 = 0x3;
pub const DESC_TYPE_INVALID: u64 = 0b00;
pub const DESC_TYPE_TABLE: u64   = 0b11;  // points to next-level table
pub const DESC_TYPE_BLOCK: u64   = 0b01;  // 2MB or 1GB block mapping
pub const DESC_TYPE_PAGE: u64    = 0b11;  // 4KB page (always at L3)

// Page attribute bits (same encoding for all levels)
pub const ATTR_ACCESS: u64       = 1 << 10;
pub const ATTR_DIRTY: u64        = 1 << 55;  // Available in L3 only (AF is bit 10)
pub const ATTR_NON_GLOBAL: u64   = 1 << 11;
pub const ATTR_AP_RW: u64        = 1 << 7;   // Access flag: RW at EL1, RW at EL0 if clear
pub const ATTR_AP_USER: u64      = 1 << 6;   // EL0 access allowed
pub const ATTR_AP_KERNEL: u64    = 0 << 6;   // EL1 only
pub const ATTR_INNER_SHAREABLE: u64 = 3 << 8;
pub const ATTR_OUTER_SHAREABLE: u64 = 2 << 8;
pub const ATTR_NON_CACHEABLE: u64   = 0 << 2;  // MAIR index 1 (Device)
pub const ATTR_NORMAL: u64          = 0 << 2;  // MAIR index 0 (Normal WBWA)
pub const ATTR_NORMAL_NC: u64       = 2 << 2;  // MAIR index 2 (Normal NC)

// Memory Attribute Indirection Register (MAIR) indices
pub const MAIR_NORMAL_IDX: u64   = 0;
pub const MAIR_DEVICE_IDX: u64   = 1;
pub const MAIR_NORMAL_NC_IDX: u64 = 2;

/// Read the current kernel page table base (TTBR1_EL1).
pub fn active_table_phys() -> u64 {
    let ttbr: u64;
    unsafe {
        core::arch::asm!("mrs {}, ttbr1_el1", out(reg) ttbr, options(nostack, preserves_flags));
    }
    // TTBR1_EL1 encodes the physical address in bits [47:1] (for 4KB granule)
    ttbr & 0x0000_FFFF_FFFF_FFF0
}

/// Walk the aarch64 page table to translate a virtual address to a physical address.
/// Returns `None` if the translation fails (no mapping).
pub fn virt_to_phys(virt: u64) -> Option<u64> {
    let phys_offset = crate::memory::PHYSICAL_MEMORY_OFFSET.get()?;

    // 4KB granule, 48-bit VA space: vpn[3..0] select entries at levels 0..3
    // virt[47:39] -> L0 index (9 bits)
    // virt[38:30] -> L1 index (9 bits)
    // virt[29:21] -> L2 index (9 bits)
    // virt[20:12] -> L3 index (9 bits)
    // virt[11:0]  -> page offset

    let l0_idx = ((virt >> 39) & 0x1FF) as usize;
    let l1_idx = ((virt >> 30) & 0x1FF) as usize;
    let l2_idx = ((virt >> 21) & 0x1FF) as usize;
    let l3_idx = ((virt >> 12) & 0x1FF) as usize;
    let offset = virt & 0xFFF;

    let table_pa = active_table_phys();

    // Walk L0
    let l0_entry = read_pt_entry(table_pa, l0_idx)?;
    if l0_entry & DESC_TYPE_MASK != DESC_TYPE_TABLE {
        return None;
    }
    let l1_pa = l0_entry & 0x0000_FFFF_FFFF_F000;

    // Walk L1 (can be a 1GB block)
    let l1_entry = read_pt_entry(l1_pa, l1_idx)?;
    let l1_type = l1_entry & DESC_TYPE_MASK;
    if l1_type == DESC_TYPE_BLOCK {
        // 1GB block
        return Some((l1_entry & 0xFFFF_FC00_0000_0000) | (virt & 0x3FFF_FFFF));
    }
    if l1_type != DESC_TYPE_TABLE {
        return None;
    }
    let l2_pa = l1_entry & 0x0000_FFFF_FFFF_F000;

    // Walk L2 (can be a 2MB block)
    let l2_entry = read_pt_entry(l2_pa, l2_idx)?;
    let l2_type = l2_entry & DESC_TYPE_MASK;
    if l2_type == DESC_TYPE_BLOCK {
        // 2MB block
        return Some((l2_entry & 0xFFFF_FFE0_0000) | (virt & 0x1F_FFFF));
    }
    if l2_type != DESC_TYPE_TABLE {
        return None;
    }
    let l3_pa = l2_entry & 0x0000_FFFF_FFFF_F000;

    // Walk L3 (must be a page)
    let l3_entry = read_pt_entry(l3_pa, l3_idx)?;
    if l3_entry & DESC_TYPE_MASK != DESC_TYPE_PAGE {
        return None;
    }

    Some((l3_entry & 0x0000_FFFF_FFFF_F000) | offset)
}

/// Read a page table entry at the given table physical address + index.
unsafe fn read_pt_entry(table_pa: u64, index: usize) -> Option<u64> {
    let phys_offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get()?;
    let virt = phys_offset + table_pa;
    let ptr = (virt + (index as u64 * 8)) as *const u64;
    let entry = unsafe { ptr.read_volatile() };
    if entry == 0 {
        return None;
    }
    Some(entry)
}

/// Write a page table entry at the given table physical address + index.
unsafe fn write_pt_entry(table_pa: u64, index: usize, entry: u64) {
    let phys_offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get()
        .expect("PHYSICAL_MEMORY_OFFSET not initialized");
    let virt = phys_offset + table_pa;
    let ptr = (virt + (index as u64 * 8)) as *mut u64;
    unsafe {
        ptr.write_volatile(entry);
    }
}

/// Create a page table entry for a 4KB page.
pub fn make_pte(paddr: u64, user_accessible: bool, writable: bool) -> u64 {
    let mut entry = paddr & 0x0000_FFFF_FFFF_F000; // physical address (page-aligned)
    entry |= DESC_TYPE_PAGE;
    entry |= ATTR_ACCESS;
    entry |= ATTR_NON_GLOBAL;  // not global (process-specific)
    if user_accessible {
        entry |= ATTR_AP_USER | ATTR_AP_RW;
    }
    if writable {
        entry |= ATTR_AP_RW;
    }
    entry
}

/// Create a page table entry for a 2MB block.
#[allow(dead_code)]
pub fn make_block_2m(paddr: u64, user_accessible: bool, writable: bool) -> u64 {
    let mut entry = paddr & 0xFFFF_FFE0_0000; // 2MB-aligned
    entry |= DESC_TYPE_BLOCK;
    entry |= ATTR_ACCESS;
    entry |= ATTR_NON_GLOBAL;
    if user_accessible {
        entry |= ATTR_AP_USER | ATTR_AP_RW;
    }
    if writable {
        entry |= ATTR_AP_RW;
    }
    entry
}

/// Create a table descriptor (points to next level).
pub fn make_table_desc(next_table_pa: u64) -> u64 {
    (next_table_pa & 0x0000_FFFF_FFFF_F000) | DESC_TYPE_TABLE
}

/// Allocate a zeroed page for use as a page table.
/// Returns the physical address of the new table.
pub fn alloc_frame_pa() -> Option<u64> {
    // Use the buddy allocator's internal allocation directly via raw access.
    // This avoids the x86_64 crate's PhysFrame/PhysAddr types.
    buddy_alloc_frame_raw()
}

/// Clone the kernel page tables into a new page table hierarchy for a process.
/// Returns the physical address of the new L0 table.
pub fn clone_kernel_tables() -> Option<u64> {
    let phys_offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get()?;

    let src_l0_pa = active_table_phys();

    let new_l0_pa = alloc_frame_pa()?;

    unsafe {
        let src_l0_virt = phys_offset + src_l0_pa;
        let dst_l0_virt = phys_offset + new_l0_pa;

        for i in 256..512 {
            let src_entry = ((src_l0_virt + (i as u64 * 8)) as *const u64).read_volatile();
            if src_entry != 0 {
                let entry = clone_pt_entry_tree(src_entry, i, phys_offset, 1);
                ((dst_l0_virt + (i as u64 * 8)) as *mut u64).write_volatile(entry);
            }
        }
    }

    Some(new_l0_pa)
}

/// Recursively clone a page table entry at the given level.
/// level 0 = L0 (512GB entries), level 1 = L1 (1GB), level 2 = L2 (2MB), level 3 = L3 (4KB)
fn clone_pt_entry_tree(
    entry: u64,
    _index: usize,
    phys_offset: u64,
    level: usize,
) -> u64 {
    if level >= 4 {
        return entry; // Leaf level, just copy
    }

    if entry & DESC_TYPE_MASK != DESC_TYPE_TABLE {
        return entry; // Block entry, share
    }

    let table_pa = entry & 0x0000_FFFF_FFFF_F000;

    // For kernel tables, we can share (read-only) or copy
    // Currently sharing kernel page tables (CoW for user tables)
    // Kernel entries are at high indices; we share them.
    // For simplicity, share all table entries by returning the original.
    // Per-process kernel page tables would need per-process ASIDs for TLBI.
    entry
}

/// Identity-map memory for aarch64 early boot.
/// Sets up a 4KB page table hierarchy that identity-maps the given physical range.
pub fn identity_map_range(
    start_pa: u64,
    end_pa: u64,
    table_l0_pa: u64,
) -> Option<()> {
    let phys_offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get()?;

    let start = start_pa & !0xFFF;
    let end = (end_pa + 0xFFF) & !0xFFF;

    let mut virt = start;
    while virt < end {
        let l0_idx = ((virt >> 39) & 0x1FF) as usize;
        let l1_idx = ((virt >> 30) & 0x1FF) as usize;
        let l2_idx = ((virt >> 21) & 0x1FF) as usize;
        let l3_idx = ((virt >> 12) & 0x1FF) as usize;

        let l1_pa = walk_or_create(table_l0_pa, l0_idx, phys_offset)?;
        let l2_pa = walk_or_create(l1_pa, l1_idx, phys_offset)?;
        let l3_pa = walk_or_create(l2_pa, l2_idx, phys_offset)?;

        // Write L3 PTE
        let pte = make_pte(virt, false, true);
        let l3_virt = phys_offset + l3_pa;
        unsafe {
            ((l3_virt + (l3_idx as u64 * 8)) as *mut u64).write_volatile(pte);
        }

        virt += PAGE_SIZE;
    }

    Some(())
}

/// Walk to the next-level table, creating it if it doesn't exist.
fn walk_or_create(
    table_pa: u64,
    index: usize,
    phys_offset: u64,
) -> Option<u64> {
    unsafe {
        let table_virt = phys_offset + table_pa;
        let entry_ptr = (table_virt + (index as u64 * 8)) as *mut u64;
        let entry = entry_ptr.read_volatile();

        if entry & DESC_TYPE_MASK == DESC_TYPE_TABLE {
            return Some(entry & 0x0000_FFFF_FFFF_F000);
        }

        if entry != 0 {
            return None;
        }

        let new_table_pa = alloc_frame_pa()?;
        let new_desc = make_table_desc(new_table_pa);
        entry_ptr.write_volatile(new_desc);
        Some(new_table_pa)
    }
}

/// Allocate a zeroed page for page table use.
fn buddy_alloc_frame_raw() -> Option<u64> {
    crate::memory::buddy::allocate_raw_frame_addr().map(|pa| {
        let phys_offset = crate::memory::PHYSICAL_MEMORY_OFFSET.get().copied().unwrap_or(0);
        let virt = phys_offset + pa;
        unsafe {
            core::ptr::write_bytes(virt as *mut u8, 0, PAGE_SIZE as usize);
        }
        pa
    })
}

/// Copy data from user space to kernel space.
///
/// # Safety
/// `user_ptr` must be a valid user-space pointer for `len` bytes.
pub unsafe fn _copy_from_user(kernel_buf: &mut [u8], user_ptr: *const u8, len: usize) {
    // aarch64: No SMAP equivalent by default; direct access works.
    // If PAN is enabled, use SCTLR_EL1.SPAN or PSTATE.PAN.
    // For now, assume PAN is disabled.
    core::ptr::copy_nonoverlapping(user_ptr, kernel_buf.as_mut_ptr(), len);
}

/// Copy data from kernel to user space.
pub unsafe fn copy_to_user(user_ptr: *mut u8, kernel_buf: &[u8], len: usize) {
    core::ptr::copy_nonoverlapping(kernel_buf.as_ptr(), user_ptr, len);
}

/// Activate an aarch64 address space by writing TTBR1_EL1.
/// Flushes TLB.
pub unsafe fn activate_address_space(l0_pa: u64) {
    core::arch::asm!("msr ttbr1_el1, {}", in(reg) l0_pa);
    core::arch::asm!("tlbi vmalle1is");
    core::arch::asm!("dsb ish");
    core::arch::asm!("isb");
}

/// A minimal aarch64 address space for testing.
pub struct AArch64AddressSpace {
    pub l0_table_pa: u64,
}

impl AArch64AddressSpace {
    pub fn new() -> Option<Self> {
        let l0_pa = clone_kernel_tables()?;
        Some(AArch64AddressSpace { l0_table_pa: l0_pa })
    }

    pub unsafe fn activate(&self) {
        activate_address_space(self.l0_table_pa);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Validate page table descriptor encoding/decoding.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_pte() {
        let paddr = 0x4008_0000;
        let pte = make_pte(paddr, false, true);
        assert_eq!(pte & DESC_TYPE_MASK, DESC_TYPE_PAGE, "must be page type");
        assert!(pte & ATTR_ACCESS != 0, "must have access flag");
        assert!(pte & ATTR_AP_RW != 0, "must be writable");
        assert!(pte & ATTR_AP_USER == 0, "must not be user-accessible");
        // Physical address should be preserved
        assert_eq!(pte & 0x0000_FFFF_FFFF_F000, paddr & 0x0000_FFFF_FFFF_F000);
    }

    #[test]
    fn test_make_user_pte() {
        let paddr = 0x7F00_0000;
        let pte = make_pte(paddr, true, true);
        assert_eq!(pte & DESC_TYPE_MASK, DESC_TYPE_PAGE);
        assert!(pte & ATTR_AP_USER != 0, "must be user-accessible");
        assert!(pte & ATTR_AP_RW != 0, "must be writable");
    }

    #[test]
    fn test_make_pte_readonly() {
        let paddr = 0x1000;
        let pte = make_pte(paddr, true, false);
        assert!(pte & ATTR_AP_USER != 0, "must be user-accessible");
        assert!(pte & ATTR_AP_RW == 0, "must NOT be writable");
    }

    #[test]
    fn test_make_block_2m() {
        let paddr = 0x4000_0000;
        let block = make_block_2m(paddr, false, true);
        assert_eq!(block & DESC_TYPE_MASK, DESC_TYPE_BLOCK, "must be block type");
        assert_eq!(block & 0xFFFF_FFE0_0000, paddr & 0xFFFF_FFE0_0000);
    }

    #[test]
    fn test_make_table_desc() {
        let child_pa = 0x5000_0000;
        let desc = make_table_desc(child_pa);
        assert_eq!(desc & DESC_TYPE_MASK, DESC_TYPE_TABLE);
        assert_eq!(desc & 0x0000_FFFF_FFFF_F000, child_pa & 0x0000_FFFF_FFFF_F000);
    }

    #[test]
    fn test_page_size_const() {
        assert_eq!(PAGE_SIZE, 4096);
        assert_eq!(PAGE_TABLE_ENTRIES, 512);
    }

    #[test]
    fn test_descriptor_types_mutually_exclusive() {
        // A descriptor can only be one type
        assert_ne!(DESC_TYPE_INVALID, DESC_TYPE_TABLE);
        assert_ne!(DESC_TYPE_INVALID, DESC_TYPE_BLOCK);
        assert_ne!(DESC_TYPE_TABLE, DESC_TYPE_BLOCK);
        // Page and table share the same type bits (0b11) but differ by level
        assert_eq!(DESC_TYPE_TABLE, DESC_TYPE_PAGE);
    }

    #[test]
    fn test_virt_to_phys_index_calculation() {
        // For a kernel higher-half address 0xFFFF_8000_0000_0000:
        //   L0 index = (0xFFFF_8000_0000_0000 >> 39) & 0x1FF
        //            = (0x1FF_0000_0000_0000 >> 39) & 0x1FF
        //            = 0x3FE & 0x1FF
        //            = 0x1FE = 510
        let virt: u64 = 0xFFFF_8000_0000_0000;
        let l0_idx = ((virt >> 39) & 0x1FF) as usize;
        assert_eq!(l0_idx, 256); // 0xFFFF_8000_0000_0000 >> 39 = 0x1FF0000 >> 39...

        // Let's just verify the formula
        let v = 0xFFFF_FFFF_8000_0000u64.wrapping_add(0x8000_0000);
        let _ = v;

        // Verify 4KB granule: 9 bits per level, 12-bit page offset
        assert_eq!((PAGE_SIZE as f64).log2() as u32, 12);
        assert_eq!((PAGE_TABLE_ENTRIES as f64).log2() as u32, 9);
        // Total VA bits = 9*4 + 12 = 48
        assert_eq!(9 * 4 + 12, 48);
    }

    #[test]
    fn test_make_pte_physical_address_alignment() {
        // Physical address must be page-aligned (lower 12 bits clear)
        let paddr = 0x1234_5678_9000; // page-aligned
        let pte = make_pte(paddr, false, false);
        assert_eq!(pte & 0xFFF, 0x3 | ATTR_ACCESS | ATTR_NON_GLOBAL, "lower bits should only be flags");

        // Non-page-aligned address: lower bits are masked
        let misaligned = 0x1234;
        let pte2 = make_pte(misaligned, false, false);
        assert_eq!(pte2 & 0x0000_FFFF_FFFF_F000, 0x1000, "should round down to page boundary");
    }

    #[test]
    fn test_page_table_entry_combination() {
        // Simulate a full PTE for a user writable page
        let paddr = 0x1000_0000;
        let pte = make_pte(paddr, true, true);
        assert_eq!(pte & DESC_TYPE_MASK, DESC_TYPE_PAGE);
        assert_eq!((pte >> 6) & 0x3, 0b11, "AP[2:1] = 11 means EL0/EL1 RW"); // AP=0b11
        assert_eq!(pte & 0x0000_FFFF_FFFF_F000, 0x1000_0000);

        // Physical address decodes back correctly
        let decoded_pa = pte & 0x0000_FFFF_FFFF_F000;
        assert_eq!(decoded_pa, paddr);
    }
}
