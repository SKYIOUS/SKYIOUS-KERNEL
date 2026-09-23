use crate::buddy::BuddyFrameAllocator;
use vahi_sync::IrqSafeMutex as Mutex;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::VirtAddr;

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
        let phys_mem_offset = VirtAddr::new(*crate::PHYSICAL_MEMORY_OFFSET.get()?);
        let level_4_table = crate::active_level_4_table(phys_mem_offset);
        x86_64::structures::paging::OffsetPageTable::new(level_4_table, phys_mem_offset)
    };

    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    // Map with rollback: if any page fails to map, free every frame claimed so
    // far. The previous early-return leaked up to size_in_pages-1 frames per
    // failed allocation, and each failed alloc_stack call bumped the bump
    // pointer permanently (fragmenting the kernel-stack VA range).
    let mut mapped: alloc::vec::Vec<(Page<Size4KiB>, PhysFrame)> = alloc::vec::Vec::new();
    for page in Page::range_inclusive(start_page, end_page) {
        let frame = match frame_allocator.allocate_frame() {
            Some(f) => f,
            None => {
                rollback(&mut mapper, &mut mapped);
                return None;
            }
        };
        // SAFETY: map_to with a freshly allocated frame and valid page range;
        // on failure we roll back everything mapped so far.
        match unsafe { mapper.map_to(page, frame, flags, &mut frame_allocator) } {
            Ok(t) => t.flush(),
            Err(_) => {
                crate::buddy::BUDDY_ALLOCATOR.lock().deallocate_frame(frame);
                rollback(&mut mapper, &mut mapped);
                return None;
            }
        }
        mapped.push((page, frame));
    }

    Some(Stack {
        top: stack_top,
        bottom: stack_bottom,
    })
}

/// K-03 rollback helper: unmap every page mapped so far and return the
/// frames to the buddy. Used only from alloc_stack's failure paths.
fn rollback(
    mapper: &mut x86_64::structures::paging::OffsetPageTable<'static>,
    mapped: &mut alloc::vec::Vec<(Page<Size4KiB>, PhysFrame)>,
) {
    for (p, f) in mapped.drain(..) {
        // p was mapped by alloc_stack above and never activated for user
        // execution; unmapping here returns ownership of f to the buddy.
        let _ = mapper.unmap(p);
        x86_64::instructions::tlb::flush(p.start_address());
        crate::buddy::BUDDY_ALLOCATOR.lock().deallocate_frame(f);
    }
}

/// Free a stack: unmap pages and return physical frames to the buddy.
pub fn free_stack(stack: &Stack) {
    let stack_size = (stack.top - stack.bottom) as usize;
    if stack_size == 0 {
        return;
    }

    let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(stack.bottom));
    let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new(stack.top - 1));

    let offset = match crate::PHYSICAL_MEMORY_OFFSET.get() {
        Some(o) => *o,
        None => return,
    };

    let mut mapper = unsafe {
        let phys_mem_offset = VirtAddr::new(offset);
        let level_4_table = crate::active_level_4_table(phys_mem_offset);
        x86_64::structures::paging::OffsetPageTable::new(level_4_table, phys_mem_offset)
    };

    for page in Page::range_inclusive(start_page, end_page) {
        if let Ok((frame, _)) = mapper.unmap(page) {
            x86_64::instructions::tlb::flush(page.start_address());
            crate::buddy::BUDDY_ALLOCATOR.lock().deallocate_frame(frame);
        }
    }
}
