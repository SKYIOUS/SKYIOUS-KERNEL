use x86_64::structures::paging::{
    Mapper, Page, PageTableFlags, Size4KiB, FrameAllocator,
};
use x86_64::VirtAddr;
use crate::memory::buddy::BuddyFrameAllocator;
use crate::sync::IrqSafeMutex as Mutex;

pub struct Stack {
    pub top: u64,
    pub bottom: u64,
}

pub fn alloc_stack(size_in_pages: usize) -> Option<Stack> {
    // Bump-allocate new virtual range. No free-list reuse: free_stack unmaps
    // the pages, so handing back an unmapped Stack would fault on switch.
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

/// Free a stack: unmap pages and return physical frames to the buddy.
pub fn free_stack(stack: &Stack) {
    let stack_size = (stack.top - stack.bottom) as usize;
    if stack_size == 0 { return; }

    let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(stack.bottom));
    let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new(stack.top - 1));

    let offset = match crate::memory::PHYSICAL_MEMORY_OFFSET.get() {
        Some(o) => *o,
        None => return,
    };

    let mut mapper = unsafe {
        let phys_mem_offset = VirtAddr::new(offset);
        let level_4_table = crate::memory::active_level_4_table(phys_mem_offset);
        x86_64::structures::paging::OffsetPageTable::new(level_4_table, phys_mem_offset)
    };

    for page in Page::range_inclusive(start_page, end_page) {
        if let Ok((frame, _)) = mapper.unmap(page) {
            x86_64::instructions::tlb::flush(page.start_address());
            crate::memory::buddy::BUDDY_ALLOCATOR.lock().deallocate_frame(frame);
        }
    }
}
