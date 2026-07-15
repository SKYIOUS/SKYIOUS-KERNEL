//! EPT (Extended Page Tables) / NPT (Nested Page Tables) manager.
//!
//! Provides a two-dimensional page table so the guest's physical addresses
//! translate to host physical addresses without hypervisor intervention.

use core::arch::asm;
use crate::memory::PHYSICAL_MEMORY_OFFSET;
use crate::memory::buddy::BUDDY_ALLOCATOR;
use x86_64::PhysAddr;

/// EPT PML4 table (4 levels for 48-bit guest physical addresses).
#[repr(C, align(4096))]
pub struct EptPml4 {
    pub entries: [u64; 512],
}

impl EptPml4 {
    /// Allocate and zero a new EPT PML4 table.
    pub fn new() -> Option<&'static mut EptPml4> {
        let offset = *PHYSICAL_MEMORY_OFFSET.get()?;
        let frame = BUDDY_ALLOCATOR.lock().allocate_contiguous(0)?;
        let virt = (frame.as_u64() + offset) as *mut EptPml4;
        // SAFETY: frame is valid, zeroed, mapped at virt.
        unsafe {
            core::ptr::write_bytes(virt as *mut u8, 0, 4096);
            Some(&mut *virt)
        }
    }

    pub fn phys_addr(&self) -> Option<u64> {
        let offset = *PHYSICAL_MEMORY_OFFSET.get()?;
        Some((self as *const EptPml4 as u64) - offset)
    }
}

/// EPT memory type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EptMemoryType {
    Uncacheable = 0,
    WriteCombining = 1,
    WriteThrough = 4,
    WriteProtected = 5,
    WriteBack = 6,
}

/// EPT entry flags.
#[repr(C)]
pub struct EptFlags {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub mem_type: EptMemoryType,
    pub ignore_pat: bool,
    pub access_dirty: bool,
    pub supervisor_shadow: bool,
}

impl EptFlags {
    pub const fn read_write() -> Self {
        EptFlags {
            read: true,
            write: true,
            execute: false,
            mem_type: EptMemoryType::WriteBack,
            ignore_pat: false,
            access_dirty: true,
            supervisor_shadow: false,
        }
    }

    pub const fn read_write_execute() -> Self {
        EptFlags {
            read: true,
            write: true,
            execute: true,
            mem_type: EptMemoryType::WriteBack,
            ignore_pat: false,
            access_dirty: true,
            supervisor_shadow: false,
        }
    }
}

/// EPT manager — owns an EPT paging structure.
pub struct EptManager {
    pub root_pml4: PhysAddr,
    pub levels: u8,
}

impl EptManager {
    pub fn new() -> Option<Self> {
        let pml4 = EptPml4::new()?;
        let phys = PhysAddr::new(pml4.phys_addr()?);
        Some(EptManager {
            root_pml4: phys,
            levels: 4,
        })
    }

    pub fn map_guest(&mut self, guest_phys: u64, host_phys: u64, size: usize, flags: EptFlags) -> bool {
        // Map region page by page (4KB)
        let mut gpa = guest_phys;
        let mut hpa = host_phys;
        let end = guest_phys + size as u64;

        while gpa < end {
            // SAFETY: self.root_pml4 is valid.
            if unsafe { ept_map_4k(self.root_pml4.as_u64(), gpa, hpa, &flags) } {
                gpa += 0x1000;
                hpa += 0x1000;
            } else {
                return false;
            }
        }
        true
    }

    pub fn unmap_guest(&mut self, guest_phys: u64) -> bool {
        let offset = match PHYSICAL_MEMORY_OFFSET.get() {
            Some(o) => *o,
            None => return false,
        };

        let pml4_idx = ((guest_phys >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((guest_phys >> 30) & 0x1FF) as usize;
        let pd_idx = ((guest_phys >> 21) & 0x1FF) as usize;
        let pt_idx = ((guest_phys >> 12) & 0x1FF) as usize;

        let pml4_virt = (self.root_pml4.as_u64() + offset) as *mut u64;
        // SAFETY: pml4_virt is valid mapped memory.
        let pdpt_phys = unsafe { *pml4_virt.add(pml4_idx) } & 0xFFFFFFF000;
        if pdpt_phys == 0 { return false; }
        let pdpt_virt = (pdpt_phys + offset) as *mut u64;
        // SAFETY: pdpt_virt is valid.
        let pd_phys = unsafe { *pdpt_virt.add(pdpt_idx) } & 0xFFFFFFF000;
        if pd_phys == 0 { return false; }
        let pd_virt = (pd_phys + offset) as *mut u64;
        // SAFETY: pd_virt is valid.
        let pt_phys = unsafe { *pd_virt.add(pd_idx) } & 0xFFFFFFF000;
        if pt_phys == 0 { return false; }
        let pt_virt = (pt_phys + offset) as *mut u64;
        // SAFETY: Clear PTE entry.
        unsafe { *pt_virt.add(pt_idx) = 0; }
        // Invalidate EPT TLB
        ept_sync();
        true
    }

    /// Handle an EPT violation by checking if the page should be mapped on demand.
    pub fn handle_ept_violation(&mut self, _gpa: u64) -> EptAction {
        // ponytail: demand paging for ballooned/overcommitted memory
        // add when memory overcommit is supported
        EptAction::InjectPageFault
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EptAction {
    Allow,
    InjectPageFault,
    MmioEmulation,
}

/// Map a 4KB page in the EPT.
///
/// # Safety
/// `pml4_pa` must point to a valid EPT PML4.
unsafe fn ept_map_4k(pml4_pa: u64, guest_phys: u64, host_phys: u64, flags: &EptFlags) -> bool {
    let offset = match PHYSICAL_MEMORY_OFFSET.get() {
        Some(o) => *o,
        None => return false,
    };

    let pml4_idx = ((guest_phys >> 39) & 0x1FF) as usize;
    let pdpt_idx = ((guest_phys >> 30) & 0x1FF) as usize;
    let pd_idx = ((guest_phys >> 21) & 0x1FF) as usize;
    let pt_idx = ((guest_phys >> 12) & 0x1FF) as usize;

    let pml4_virt = (pml4_pa + offset) as *mut u64;

    // Ensure PDPT entry exists
    // SAFETY: pml4_virt is valid.
    if unsafe { *pml4_virt.add(pml4_idx) } & 1 == 0 {
        let pdpt_frame = match BUDDY_ALLOCATOR.lock().allocate_contiguous(0) {
            Some(f) => f,
            None => return false,
        };
        let pdpt_virt = (pdpt_frame.as_u64() + offset) as *mut u64;
        // SAFETY: allocated frame is writable.
        unsafe { core::ptr::write_bytes(pdpt_virt, 0, 4096); }
        // SAFETY: write back to PML4 entry.
        unsafe { *pml4_virt.add(pml4_idx) = pdpt_frame.as_u64() | 0x7; }
    }

    // SAFETY: read PDPT entry.
    let pdpt_phys = unsafe { *pml4_virt.add(pml4_idx) } & 0xFFFFFFF000;
    let pdpt_virt = (pdpt_phys + offset) as *mut u64;

    // Ensure PD entry exists
    // SAFETY: pdpt_virt is valid.
    if unsafe { *pdpt_virt.add(pdpt_idx) } & 1 == 0 {
        let pd_frame = match BUDDY_ALLOCATOR.lock().allocate_contiguous(0) {
            Some(f) => f,
            None => return false,
        };
        let pd_virt = (pd_frame.as_u64() + offset) as *mut u64;
        // SAFETY: allocated frame is writable.
        unsafe { core::ptr::write_bytes(pd_virt, 0, 4096); }
        // SAFETY: write back to PDPT entry.
        unsafe { *pdpt_virt.add(pdpt_idx) = pd_frame.as_u64() | 0x7; }
    }

    // SAFETY: read PD entry.
    let pd_phys = unsafe { *pdpt_virt.add(pdpt_idx) } & 0xFFFFFFF000;
    let pd_virt = (pd_phys + offset) as *mut u64;

    // Ensure PT entry exists
    // SAFETY: pd_virt is valid.
    if unsafe { *pd_virt.add(pd_idx) } & 1 == 0 {
        let pt_frame = match BUDDY_ALLOCATOR.lock().allocate_contiguous(0) {
            Some(f) => f,
            None => return false,
        };
        let pt_virt = (pt_frame.as_u64() + offset) as *mut u64;
        // SAFETY: allocated frame is writable.
        unsafe { core::ptr::write_bytes(pt_virt, 0, 4096); }
        // SAFETY: write back to PD entry.
        unsafe { *pd_virt.add(pd_idx) = pt_frame.as_u64() | 0x7; }
    }

    // SAFETY: read PT base.
    let pt_phys = unsafe { *pd_virt.add(pd_idx) } & 0xFFFFFFF000;
    let pt_virt = (pt_phys + offset) as *mut u64;

    // Set 4KB page entry
    let mut entry = host_phys | 0x7; // Read + Write + Present
    if !flags.read { entry &= !0x1; }
    if !flags.write { entry &= !0x2; }
    if !flags.execute { entry |= 0x100; } // XD (execute-disable)
    // Set memory type (bits 3:5)
    entry |= (flags.mem_type as u64) << 3;
    if flags.ignore_pat { entry |= 1 << 6; }
    if flags.access_dirty { entry |= 1 << 8; } // Access bit tracking

    // SAFETY: write PTE.
    unsafe { *pt_virt.add(pt_idx) = entry; }

    true
}

/// Map a 2MB large page in the EPT.
///
/// # Safety
/// `pml4` must point to a valid EPT PML4 table.
pub unsafe fn ept_map_2mb(pml4: &mut EptPml4, guest_phys: u64, host_phys: u64, writable: bool, executable: bool) -> bool {
    let offset = match PHYSICAL_MEMORY_OFFSET.get() {
        Some(o) => *o,
        None => return false,
    };

    let pml4_idx = ((guest_phys >> 39) & 0x1FF) as usize;
    let pdpt_idx = ((guest_phys >> 30) & 0x1FF) as usize;
    let pd_idx = ((guest_phys >> 21) & 0x1FF) as usize;

    // Ensure PDPT entry exists
    if pml4.entries[pml4_idx] & 1 == 0 {
        let pdpt_frame = match BUDDY_ALLOCATOR.lock().allocate_contiguous(0) {
            Some(f) => f,
            None => return false,
        };
        let pdpt_virt = (pdpt_frame.as_u64() + offset) as *mut u64;
        // SAFETY: pdpt_virt is a valid writeable mapping.
        unsafe {
            core::ptr::write_bytes(pdpt_virt, 0, 4096);
        }
        pml4.entries[pml4_idx] = pdpt_frame.as_u64() | 0x7;
    }

    let pdpt_phys = pml4.entries[pml4_idx] & 0xFFFFFFF000;
    let pdpt_virt = (pdpt_phys + offset) as *mut u64;

    // Ensure PD entry exists
    // SAFETY: pdpt_virt points to valid mapped memory.
    if unsafe { *pdpt_virt.add(pdpt_idx) } & 1 == 0 {
        let pd_frame = match BUDDY_ALLOCATOR.lock().allocate_contiguous(0) {
            Some(f) => f,
            None => return false,
        };
        let pd_virt = (pd_frame.as_u64() + offset) as *mut u64;
        // SAFETY: pd_virt is a valid writeable mapping.
        unsafe {
            core::ptr::write_bytes(pd_virt, 0, 4096);
        }
        // SAFETY: pdpt_virt is a valid mapping.
        unsafe { *pdpt_virt.add(pdpt_idx) = pd_frame.as_u64() | 0x7; }
    }

    // SAFETY: pdpt_virt is a valid mapped pointer.
    let pd_phys = unsafe { *pdpt_virt.add(pdpt_idx) } & 0xFFFFFFF000;
    let pd_virt = (pd_phys + offset) as *mut u64;

    // Set 2MB large page entry (Present + writable + large page)
    let mut entry = host_phys | 0x87;
    if !writable { entry &= !0x2; }
    if !executable { entry |= 0x1000000000000000; }
    // SAFETY: pd_virt points to the page directory entry slot.
    unsafe { *pd_virt.add(pd_idx) = entry; }

    true
}

/// Synchronize EPT TLB (INVEPT).
pub fn ept_sync() {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: INVEPT with type 1 (single context) and descriptor at 0 = global.
    unsafe {
        let desc: u64 = 0;
        asm!("invept {0}, [{1}]", in(reg) 1u64, in(reg) &desc, options(nostack));
    }
}
