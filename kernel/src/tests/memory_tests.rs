use crate::memory::buddy::BuddyFrameAllocator;
use crate::memory::buddy::BUDDY_ALLOCATOR;
use crate::memory::paging::AddressSpace;
use crate::memory::phys;

fn test_phys_alloc_many() -> Result<(), &'static str> {
    if !phys::is_initialized() {
        return Err("phys allocator not initialized");
    }
    let mut frames = [0u64; 8];
    for f in frames.iter_mut() {
        *f = phys::alloc_frame().ok_or("alloc failed")?;
    }
    for i in 0..frames.len() {
        if frames[i] & 0xFFF != 0 {
            return Err("non-aligned address");
        }
        for j in i + 1..frames.len() {
            if frames[i] == frames[j] {
                return Err("duplicate addresses");
            }
        }
        phys::free_frame(frames[i]);
    }
    Ok(())
}

fn test_phys_alloc_free_reuse() -> Result<(), &'static str> {
    if !phys::is_initialized() {
        return Err("not initialized");
    }
    let f1 = phys::alloc_frame().ok_or("alloc failed")?;
    phys::free_frame(f1);
    if !phys::is_free(f1) {
        return Err("frame not free after free");
    }
    let f2 = phys::alloc_frame().ok_or("re-alloc failed")?;
    if f1 != f2 {
        return Err("expected same frame after free+realloc");
    }
    phys::free_frame(f2);
    Ok(())
}

fn test_phys_double_free_detected() -> Result<(), &'static str> {
    if !phys::is_initialized() {
        return Err("not initialized");
    }
    let f = phys::alloc_frame().ok_or("alloc failed")?;
    phys::free_frame(f);
    let before = phys::total_free_frames();
    phys::free_frame(f);
    let after = phys::total_free_frames();
    if after != before {
        return Err("double free should not change free count");
    }
    Ok(())
}

fn test_buddy_alloc_frame() -> Result<(), &'static str> {
    if !phys::is_initialized() {
        return Err("not initialized");
    }
    let mut buddy = BUDDY_ALLOCATOR.lock();
    let before = buddy.count_free_pages();
    let frame = buddy.allocate_frame().ok_or("buddy allocate_frame None")?;
    let after = buddy.count_free_pages();
    if after != before - 1 {
        return Err("free count should decrease by 1");
    }
    let addr = frame.start_address();
    if addr.as_u64() & 0xFFF != 0 {
        return Err("non-4K-aligned address");
    }
    buddy.deallocate_frame(frame);
    let after_free = buddy.count_free_pages();
    if after_free != before {
        return Err("free count not restored after dealloc");
    }
    Ok(())
}

fn test_buddy_alloc_order() -> Result<(), &'static str> {
    if !phys::is_initialized() {
        return Err("not initialized");
    }
    let mut buddy = BUDDY_ALLOCATOR.lock();
    let before = buddy.count_free_pages();
    let addr = buddy.allocate_contiguous(2);
    if addr.is_none() {
        return Err("buddy allocate_contiguous order 2 failed");
    }
    let a = addr.unwrap();
    if a.as_u64() & 0xFFF != 0 {
        return Err("non-4K-aligned");
    }
    let after = buddy.count_free_pages();
    if after != before - 4 {
        return Err("order 2 should consume 4 pages");
    }
    buddy.deallocate_contiguous(a, 2);
    let after_free = buddy.count_free_pages();
    if after_free != before {
        return Err("free count not restored after dealloc order 2");
    }
    Ok(())
}

fn test_buddy_count_free() -> Result<(), &'static str> {
    if !phys::is_initialized() {
        return Err("not initialized");
    }
    let buddy = BUDDY_ALLOCATOR.lock();
    let count = buddy.count_free_pages();
    if count == 0 {
        return Err("free page count should be > 0");
    }
    Ok(())
}

fn test_buddy_alloc_order1() -> Result<(), &'static str> {
    if !phys::is_initialized() {
        return Err("not initialized");
    }
    let mut buddy = BUDDY_ALLOCATOR.lock();
    let before = buddy.count_free_pages();
    let addr = buddy.allocate_contiguous(1).ok_or("buddy order1 failed")?;
    if addr.as_u64() & 0x1FFF != 0 {
        return Err("non-8K-aligned");
    }
    let after = buddy.count_free_pages();
    if after != before - 2 {
        return Err("order 1 should consume 2 pages");
    }
    buddy.deallocate_contiguous(addr, 1);
    if buddy.count_free_pages() != before {
        return Err("free count not restored after order1");
    }
    Ok(())
}

fn test_refcount_increment_decrement() -> Result<(), &'static str> {
    use crate::memory::phys;
    use x86_64::PhysAddr;
    if !phys::is_initialized() {
        return Err("not initialized");
    }
    let f = phys::alloc_frame().ok_or("alloc failed")?;
    let pa = PhysAddr::new(f);
    // Refcounts start at 0; increment/decrement are called by the mapping layer
    let count_before = crate::memory::frame_info::count(pa);
    crate::memory::frame_info::increment(pa);
    let count_after_inc = crate::memory::frame_info::count(pa);
    if count_after_inc != count_before + 1 {
        return Err("refcount should increase by 1 after increment");
    }
    let remaining = crate::memory::frame_info::decrement(pa);
    if remaining != count_before {
        return Err("refcount should return to original after decrement");
    }
    phys::free_frame(f);
    Ok(())
}

fn test_address_space_multiple_cow_clones() -> Result<(), &'static str> {
    let mut fa = BuddyFrameAllocator;
    let original = AddressSpace::new(&mut fa).ok_or("original failed")?;
    // Create multiple CoW clones — each should succeed
    let _clone1 = original.clone_cow(&mut fa).ok_or("clone1 failed")?;
    let _clone2 = original.clone_cow(&mut fa).ok_or("clone2 failed")?;
    let _clone3 = original.clone_cow(&mut fa).ok_or("clone3 failed")?;
    Ok(())
}

pub fn register() {
    crate::selftest::register("phys::alloc_many", test_phys_alloc_many);
    crate::selftest::register("phys::alloc_free_reuse", test_phys_alloc_free_reuse);
    crate::selftest::register("phys::double_free_detected", test_phys_double_free_detected);
    crate::selftest::register("buddy::alloc_frame", test_buddy_alloc_frame);
    crate::selftest::register("buddy::alloc_order2", test_buddy_alloc_order);
    crate::selftest::register("buddy::count_free", test_buddy_count_free);
    crate::selftest::register("buddy::alloc_order1", test_buddy_alloc_order1);
    crate::selftest::register("memory::refcount_ops", test_refcount_increment_decrement);
    crate::selftest::register(
        "address_space:multi_cow",
        test_address_space_multiple_cow_clones,
    );
    crate::selftest::register("mmap::regions_distinct", test_mmap_regions_distinct);
}

/// Regression: anonymous mmap must return a distinct region per call.
/// A fixed return address makes every mapping alias one page, corrupting
/// userspace heaps (observed: init's heap Strings all landed at 0x20000000).
fn test_mmap_regions_distinct() -> Result<(), &'static str> {
    use crate::task::process::{find_free_vma_region, Vma};
    use x86_64::structures::paging::PageTableFlags;

    let vma = |start: u64, end: u64| Vma {
        start,
        end,
        flags: PageTableFlags::PRESENT,
        _name: "selftest",
        file_handle: None,
        file_offset: 0,
        is_shared: false,
        shm_id: None,
    };
    let max = 0x7000_0000_0000u64;
    let hint = 0x4000_0000u64;
    let len = 0x1000u64;

    // Two sequential mappings must not alias.
    let mut vmas: alloc::vec::Vec<Vma> = alloc::vec::Vec::new();
    let a = find_free_vma_region(&vmas, hint, len, max).ok_or("no first region")?;
    vmas.push(vma(a, a + len));
    let b = find_free_vma_region(&vmas, hint, len, max).ok_or("no second region")?;
    if b == a {
        return Err("consecutive mmaps aliased the same region");
    }

    // A scan must skip past an existing VMA that contains the hint.
    vmas.clear();
    vmas.push(vma(hint - 0x800, hint + 0x800));
    let c = find_free_vma_region(&vmas, hint, len, max).ok_or("no post-overlap region")?;
    if c < hint + 0x800 {
        return Err("scan did not skip past an overlapping VMA");
    }

    // No space below the ceiling -> None.
    if find_free_vma_region(&vmas, max - 1, len, max).is_some() {
        return Err("scan returned a region past max_addr");
    }
    Ok(())
}
