//! Isolate-based virtual memory framework.
//!
//! An `Isolate` wraps an `AddressSpace` with region tracking, backing store
//! metadata, and a user-heap bump allocator.  Each process owns one Isolate;
//! fork creates a CoW clone via `AddressSpace::clone_cow`.

use alloc::collections::BTreeMap;
use x86_64::VirtAddr;
use x86_64::structures::paging::{Page, Size4KiB, PageTableFlags, Mapper, FrameAllocator};
use crate::memory::paging::AddressSpace;

// ── Flags & backing store ─────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmFlags(u32);

impl VmFlags {
    pub const READ:   Self = Self(0x1);
    pub const WRITE:  Self = Self(0x2);
    pub const EXEC:   Self = Self(0x4);
    pub const USER:   Self = Self(0x8);
    pub const SHARED: Self = Self(0x10);

    pub fn contains(self, other: Self) -> bool { (self.0 & other.0) == other.0 }
    pub fn bits(self) -> u32 { self.0 }
}

impl core::ops::BitOr for VmFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self { Self(self.0 | rhs.0) }
}

impl core::ops::BitOrAssign for VmFlags {
    fn bitor_assign(&mut self, rhs: Self) { self.0 |= rhs.0; }
}

impl VmFlags {
    pub fn to_page_table_flags(self) -> PageTableFlags {
        let mut f = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        if self.contains(VmFlags::WRITE)  { f.insert(PageTableFlags::WRITABLE); }
        if self.contains(VmFlags::EXEC)   { /* NX cleared by default */ }
        if !self.contains(VmFlags::EXEC)  { f.insert(PageTableFlags::NO_EXECUTE); }
        f
    }
}

#[derive(Debug, Clone)]
pub enum BackingStore {
    Anonymous,
    File { inode: u64, offset: u64 },
    Shared(u64),
}

// ── VmRegion ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VmRegion {
    pub base: u64,
    pub size: usize,
    pub flags: VmFlags,
    pub backing: BackingStore,
}

// ── Isolate ───────────────────────────────────────────────────────

pub struct Isolate {
    pub id: u64,
    pub address_space: AddressSpace,
    /// Regions keyed by base virtual address.
    pub regions: BTreeMap<u64, VmRegion>,
    /// User heap: bump-allocated from heap_base upward.
    pub heap_base: u64,
    pub heap_end: u64,
    /// Next user virtual address for anonymous mmap (grows downward from top).
    pub mmap_top: u64,
}

/// User address space layout (canonical x86_64 lower half).
const USER_MMAP_TOP: u64 = 0x0000_7FFF_FFFF_0000; // 128 TiB - 64 KiB
const USER_HEAP_BASE: u64 = 0x0000_0040_0000_0000; // 256 GiB

impl Isolate {
    /// Create a fresh isolate for a new process.
    pub fn new(id: u64, frame_allocator: &mut impl FrameAllocator<Size4KiB>) -> Option<Self> {
        Some(Isolate {
            id,
            address_space: AddressSpace::new(frame_allocator)?,
            regions: BTreeMap::new(),
            heap_base: USER_HEAP_BASE,
            heap_end: USER_HEAP_BASE,
            mmap_top: USER_MMAP_TOP,
        })
    }

    // ── mmap ──────────────────────────────────────────────────────

    /// Map `page_count` anonymous pages at a given virtual address (must be page-aligned
    /// and not overlap an existing region).  Returns the base virtual address.
    pub fn mmap_at(
        &mut self,
        virt: u64,
        size: usize,
        flags: VmFlags,
        backing: BackingStore,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> Option<u64> {
        let page_count = (size + 4095) / 4096;
        let page_bytes = page_count * 4096;
        if self.overlaps(virt, page_bytes) { return None; }

        let pt_flags = flags.to_page_table_flags();
        let mut mapper = unsafe { self.address_space.mapper()? };

        for i in 0..page_count as u64 {
            let page = Page::<Size4KiB>::containing_address(VirtAddr::new(virt + i * 4096));
            let frame = frame_allocator.allocate_frame()?;
            unsafe {
                mapper.map_to(page, frame, pt_flags, frame_allocator)
                    .ok()?
                    .ignore();
            }
        }

        self.regions.insert(virt, VmRegion { base: virt, size: page_bytes, flags, backing });
        Some(virt)
    }

    /// Map anonymous pages; the isolate picks a free address (top-down).
    pub fn mmap_anonymous(
        &mut self,
        size: usize,
        flags: VmFlags,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> Option<u64> {
        let page_count = (size + 4095) / 4096;
        let page_bytes = (page_count * 4096) as u64;
        let base = self.mmap_top.checked_sub(page_bytes)?;
        let result = self.mmap_at(base, size, flags, BackingStore::Anonymous, frame_allocator);
        if result.is_some() {
            self.mmap_top = base;
        }
        result
    }

    // ── munmap ────────────────────────────────────────────────────

    /// Unmap a region and free its physical frames.
    pub fn munmap(&mut self, virt: u64, size: usize) -> Option<VmRegion> {
        let region = self.regions.remove(&virt)?;
        let page_count = (size + 4095) / 4096;

        if let Some(mut mapper) = unsafe { self.address_space.mapper() } {
            for i in 0..page_count as u64 {
                let page = Page::<Size4KiB>::containing_address(VirtAddr::new(virt + i * 4096));
                if let Ok((frame, _flags)) = mapper.unmap(page) {
                    crate::memory::frame_info::decrement(frame.start_address());
                    crate::memory::buddy::BUDDY_ALLOCATOR.lock().deallocate_frame(frame);
                }
            }
        }
        Some(region)
    }

    // ── mprotect ──────────────────────────────────────────────────

    /// Change protection flags on an existing region.
    pub fn mprotect(&mut self, virt: u64, new_flags: VmFlags) -> bool {
        let Some(region) = self.regions.get_mut(&virt) else { return false };
        region.flags = new_flags;
        let pt_flags = new_flags.to_page_table_flags();
        let page_count = (region.size + 4095) / 4096;

        if let Some(mut mapper) = unsafe { self.address_space.mapper() } {
            for i in 0..page_count as u64 {
                let page = Page::<Size4KiB>::containing_address(VirtAddr::new(virt + i * 4096));
                unsafe { mapper.update_flags(page, pt_flags).ok(); }
            }
        }
        true
    }

    // ── heap (brk) ────────────────────────────────────────────────

    /// Extend or shrink the heap to `new_end`.  Allocates pages on demand.
    pub fn brk(&mut self, new_end: u64, frame_allocator: &mut impl FrameAllocator<Size4KiB>) -> bool {
        if new_end < self.heap_base { return false; }
        let flags = VmFlags::READ | VmFlags::WRITE | VmFlags::USER;

        while self.heap_end < new_end {
            let page = Page::<Size4KiB>::containing_address(VirtAddr::new(self.heap_end));
            let frame = match frame_allocator.allocate_frame() {
                Some(f) => f,
                None => return false,
            };
            if let Some(mut mapper) = unsafe { self.address_space.mapper() } {
                unsafe {
                    let pt_flags = flags.to_page_table_flags();
                    mapper.map_to(page, frame, pt_flags, frame_allocator)
                        .ok().map(|r| r.ignore());
                }
            }
            self.heap_end += 4096;
        }
        // Note: we don't unmap pages when shrinking — that would require
        // a TLB flush and is rarely needed.
        true
    }

    // ── fork (CoW clone) ──────────────────────────────────────────

    /// Clone this isolate for fork.  User pages are CoW-shared via
    /// `AddressSpace::clone_cow`; regions are deep-copied.
    pub fn clone_cow(
        &self,
        new_id: u64,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> Option<Self> {
        Some(Isolate {
            id: new_id,
            address_space: self.address_space.clone_cow(frame_allocator)?,
            regions: self.regions.clone(),
            heap_base: self.heap_base,
            heap_end: self.heap_end,
            mmap_top: self.mmap_top,
        })
    }

    // ── teardown ──────────────────────────────────────────────────

    /// Destroy this isolate: free all user pages and page tables.
    pub fn destroy(&self) {
        self.address_space.destroy();
    }

    // ── helpers ───────────────────────────────────────────────────

    fn overlaps(&self, base: u64, size: usize) -> bool {
        let end = base + size as u64;
        for (_, r) in self.regions.range(..end) {
            if r.base < end && base < r.base + r.size as u64 {
                return true;
            }
        }
        false
    }
}
