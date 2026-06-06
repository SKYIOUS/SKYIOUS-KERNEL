use x86_64::structures::paging::{
    Mapper, Page, PageTableFlags, Size4KiB, FrameAllocator,
};
use x86_64::VirtAddr;
use crate::memory::buddy::BuddyFrameAllocator;
use alloc::collections::VecDeque;
use spin::Mutex;

pub struct Stack {
    pub top: u64,
    pub bottom: u64,
}

/// Free list of deallocated stacks (virtual address reuse)
static STACK_FREE_LIST: Mutex<VecDeque<Stack>> = Mutex::new(VecDeque::new());

pub fn alloc_stack(size_in_pages: usize) -> Option<Stack> {
    // Check free list first for matching-size stacks
    {
        let mut free = STACK_FREE_LIST.lock();
        if let Some(idx) = free.iter().position(|s| (s.top - s.bottom) as usize == size_in_pages * 4096) {
            return Some(free.remove(idx).unwrap());
        }
    }

    // Bump-allocate new virtual range
    static NEXT_STACK_TOP: Mutex<u64> = Mutex::new(0xFFFF_E000_0000_0000);
    
    let stack_size = size_in_pages as u64 * 4096;
    
    let mut top = NEXT_STACK_TOP.lock();
    let stack_top = *top;
    let stack_bottom = stack_top - stack_size;
    let guard_page_addr = stack_bottom - 4096;
    
    *top = guard_page_addr;
    
    let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(stack_bottom));
    let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new(stack_top - 1));
    
    let mut frame_allocator = BuddyFrameAllocator;
    let mut mapper = unsafe { 
        let phys_mem_offset = VirtAddr::new(*crate::memory::PHYSICAL_MEMORY_OFFSET.get()?);
        let level_4_table = crate::memory::active_level_4_table(phys_mem_offset);
        x86_64::structures::paging::OffsetPageTable::new(level_4_table, phys_mem_offset)
    };

    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    for page in Page::range_inclusive(start_page, end_page) {
        let frame = frame_allocator.allocate_frame()?;
        unsafe {
            if let Ok(t) = mapper.map_to(page, frame, flags, &mut frame_allocator) {
                t.flush();
            } else {
                return None;
            }
        }
    }

    Some(Stack {
        top: stack_top,
        bottom: stack_bottom,
    })
}

/// Free a stack: unmap pages, free physical frames, and return virtual range
/// to the free list for reuse.
pub fn free_stack(stack: &Stack) {
    let stack_size = (stack.top - stack.bottom) as usize;
    if stack_size == 0 { return; }

    let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(stack.bottom));
    let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new(stack.top - 1));

    let mut mapper = unsafe { 
        if let Some(offset) = crate::memory::PHYSICAL_MEMORY_OFFSET.get() {
            let phys_mem_offset = VirtAddr::new(*offset);
            let level_4_table = crate::memory::active_level_4_table(phys_mem_offset);
            x86_64::structures::paging::OffsetPageTable::new(level_4_table, phys_mem_offset)
        } else {
            return;
        }
    };

    for page in Page::range_inclusive(start_page, end_page) {
        if let Ok((frame, _)) = mapper.unmap(page) {
            x86_64::instructions::tlb::flush(page.start_address());
            crate::memory::buddy::BUDDY_ALLOCATOR.lock().deallocate_frame(frame);
        }
    }

    STACK_FREE_LIST.lock().push_back(Stack {
        top: stack.top,
        bottom: stack.bottom,
    });
}
