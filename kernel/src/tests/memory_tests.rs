use crate::memory::buddy::BUDDY_ALLOCATOR;
use crate::memory::phys;

fn test_phys_alloc_many() -> Result<(), &'static str> {
    if !phys::is_initialized() { return Err("phys allocator not initialized"); }
    let mut frames = [0u64; 8];
    for f in frames.iter_mut() {
        *f = phys::alloc_frame().ok_or("alloc failed")?;
    }
    for i in 0..frames.len() {
        if frames[i] & 0xFFF != 0 { return Err("non-aligned address"); }
        for j in i + 1..frames.len() {
            if frames[i] == frames[j] { return Err("duplicate addresses"); }
        }
        phys::free_frame(frames[i]);
    }
    Ok(())
}

fn test_phys_alloc_free_reuse() -> Result<(), &'static str> {
    if !phys::is_initialized() { return Err("not initialized"); }
    let f1 = phys::alloc_frame().ok_or("alloc failed")?;
    phys::free_frame(f1);
    if !phys::is_free(f1) { return Err("frame not free after free"); }
    let f2 = phys::alloc_frame().ok_or("re-alloc failed")?;
    if f1 != f2 { return Err("expected same frame after free+realloc"); }
    phys::free_frame(f2);
    Ok(())
}

fn test_phys_double_free_detected() -> Result<(), &'static str> {
    if !phys::is_initialized() { return Err("not initialized"); }
    let f = phys::alloc_frame().ok_or("alloc failed")?;
    phys::free_frame(f);
    let before = phys::total_free_frames();
    phys::free_frame(f);
    let after = phys::total_free_frames();
    if after != before { return Err("double free should not change free count"); }
    Ok(())
}

fn test_buddy_alloc_frame() -> Result<(), &'static str> {
    if !phys::is_initialized() { return Err("not initialized"); }
    let mut buddy = BUDDY_ALLOCATOR.lock();
    let before = buddy.count_free_pages();
    let frame = buddy.allocate_frame().ok_or("buddy allocate_frame None")?;
    let after = buddy.count_free_pages();
    if after != before - 1 { return Err("free count should decrease by 1"); }
    let addr = frame.start_address();
    if addr.as_u64() & 0xFFF != 0 { return Err("non-4K-aligned address"); }
    buddy.deallocate_frame(frame);
    let after_free = buddy.count_free_pages();
    if after_free != before { return Err("free count not restored after dealloc"); }
    Ok(())
}

fn test_buddy_alloc_order() -> Result<(), &'static str> {
    if !phys::is_initialized() { return Err("not initialized"); }
    let mut buddy = BUDDY_ALLOCATOR.lock();
    let before = buddy.count_free_pages();
    let addr = buddy.allocate_contiguous(2);
    if addr.is_none() { return Err("buddy allocate_contiguous order 2 failed"); }
    let a = addr.unwrap();
    if a.as_u64() & 0xFFF != 0 { return Err("non-4K-aligned"); }
    let after = buddy.count_free_pages();
    if after != before - 4 { return Err("order 2 should consume 4 pages"); }
    buddy.deallocate_contiguous(a, 2);
    let after_free = buddy.count_free_pages();
    if after_free != before { return Err("free count not restored after dealloc order 2"); }
    Ok(())
}

fn test_buddy_count_free() -> Result<(), &'static str> {
    if !phys::is_initialized() { return Err("not initialized"); }
    let buddy = BUDDY_ALLOCATOR.lock();
    let count = buddy.count_free_pages();
    if count == 0 { return Err("free page count should be > 0"); }
    Ok(())
}

fn test_buddy_alloc_order1() -> Result<(), &'static str> {
    if !phys::is_initialized() { return Err("not initialized"); }
    let mut buddy = BUDDY_ALLOCATOR.lock();
    let before = buddy.count_free_pages();
    let addr = buddy.allocate_contiguous(1).ok_or("buddy order1 failed")?;
    if addr.as_u64() & 0x1FFF != 0 { return Err("non-8K-aligned"); }
    let after = buddy.count_free_pages();
    if after != before - 2 { return Err("order 1 should consume 2 pages"); }
    buddy.deallocate_contiguous(addr, 1);
    if buddy.count_free_pages() != before { return Err("free count not restored after order1"); }
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
}
