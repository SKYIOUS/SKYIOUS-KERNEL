use x86_64::{
    structures::paging::{PhysFrame, Size4KiB, FrameAllocator},
    PhysAddr, VirtAddr,
};
use spin::Mutex;
use crate::memory::PHYSICAL_MEMORY_OFFSET;

pub const MAX_ORDER: usize = 11; // Blocks up to 2^11 * 4096 = 8MB

pub struct BuddyAllocator {
    free_lists: [Option<PhysAddr>; MAX_ORDER + 1],
}

impl BuddyAllocator {
    pub const fn new() -> Self {
        BuddyAllocator {
            free_lists: [None; MAX_ORDER + 1],
        }
    }

    pub unsafe fn add_region(&mut self, start: PhysAddr, end: PhysAddr) {
        let mut current = start.as_u64();
        let end = end.as_u64();

        while current < end {
            // Find max order that fits alignment and size
            let remaining = end - current;
            let mut order = 0;
            while order < MAX_ORDER {
                let block_size = 4096 << (order + 1);
                if current % block_size == 0 && block_size <= remaining {
                    order += 1;
                } else {
                    break;
                }
            }
            self.add_block(PhysAddr::new(current), order);
            current += 4096 << order;
        }
    }

    pub fn allocate_frame(&mut self) -> Option<PhysFrame> {
        self.allocate_contiguous(0).map(PhysFrame::containing_address)
    }

    pub fn allocate_contiguous(&mut self, order: usize) -> Option<PhysAddr> {
        self.allocate_at_order(order)
    }

    #[allow(dead_code)]
    pub fn deallocate_contiguous(&mut self, addr: PhysAddr, order: usize) {
        self.deallocate_at_order(addr, order);
    }

    fn allocate_at_order(&mut self, order: usize) -> Option<PhysAddr> {
        if order > MAX_ORDER {
            return None;
        }

        if let Some(addr) = self.free_lists[order] {
            self.free_lists[order] = self.read_next_ptr(addr);
            return Some(addr);
        }

        // Split from higher order
        let addr = self.allocate_at_order(order + 1)?;
        let buddy_addr = PhysAddr::new(addr.as_u64() + (4096 << order));
        self.add_block(buddy_addr, order);
        Some(addr)
    }

    pub fn deallocate_frame(&mut self, frame: PhysFrame) {
        self.deallocate_at_order(frame.start_address(), 0);
    }

    pub fn deallocate_at_order(&mut self, addr: PhysAddr, order: usize) {
        if order >= MAX_ORDER {
            self.add_block(addr, order);
            return;
        }

        let block_size = 4096 << order;
        let buddy_addr = PhysAddr::new(addr.as_u64() ^ block_size as u64);

        if self.remove_block(buddy_addr, order) {
            let merged_addr = if buddy_addr < addr { buddy_addr } else { addr };
            self.deallocate_at_order(merged_addr, order + 1);
        } else {
            // Buddy not free, just add this block back
            self.add_block(addr, order);
        }
    }

    fn remove_block(&mut self, addr: PhysAddr, order: usize) -> bool {
        let mut current = self.free_lists[order];
        let mut prev: Option<PhysAddr> = None;

        while let Some(curr_addr) = current {
            let next = self.read_next_ptr(curr_addr);
            if curr_addr == addr {
                if let Some(p) = prev {
                    self.write_next_ptr(p, next);
                } else {
                    self.free_lists[order] = next;
                }
                return true;
            }
            prev = Some(curr_addr);
            current = next;
        }
        false
    }

    fn add_block(&mut self, addr: PhysAddr, order: usize) {
        let next = self.free_lists[order];
        self.write_next_ptr(addr, next);
        self.free_lists[order] = Some(addr);
    }

    pub fn count_free_pages(&self) -> usize {
        let mut total = 0usize;
        for order in 0..=MAX_ORDER {
            let mut addr = self.free_lists[order];
            while let Some(a) = addr {
                total += 1 << order;
                addr = self.read_next_ptr(a);
            }
        }
        total
    }

    fn read_next_ptr(&self, addr: PhysAddr) -> Option<PhysAddr> {
        let offset = *PHYSICAL_MEMORY_OFFSET.get().expect("Memory offset not init");
        // We use a magic value to represent None because 0 might be a valid physical address
        // But for Vahi we usually don't use address 0 for free blocks.
        // Let's use 0 as None for now, but be careful.
        let virt = VirtAddr::new(addr.as_u64() + offset);
        let ptr = virt.as_ptr::<u64>();
        let val = unsafe { *ptr };
        if val == 0 {
            None
        } else {
            // We store (phys_addr + 1) to distinguish from 0
            Some(PhysAddr::new(val - 1))
        }
    }

    fn write_next_ptr(&self, addr: PhysAddr, next: Option<PhysAddr>) {
        let offset = *PHYSICAL_MEMORY_OFFSET.get().expect("Memory offset not init");
        let virt = VirtAddr::new(addr.as_u64() + offset);
        let ptr = virt.as_mut_ptr::<u64>();
        let val = next.map(|a| a.as_u64() + 1).unwrap_or(0);
        unsafe { *ptr = val; }
    }
}

pub static BUDDY_ALLOCATOR: Mutex<BuddyAllocator> = Mutex::new(BuddyAllocator::new());

pub struct BuddyFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for BuddyFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        BUDDY_ALLOCATOR.lock().allocate_frame()
    }
}
