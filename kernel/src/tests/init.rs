//! Early-boot allocator smoke tests (runs before selftest framework).

#[cfg(feature = "self_test")]
pub fn test_memory_allocations() {
    crate::vga_buffer::set_color(
        crate::vga_buffer::Color::LightCyan,
        crate::vga_buffer::Color::Black,
    );
    crate::serial_write("\n[ SYSTEM ] Verifying Memory Allocators...\n");

    // 1. Test Small Allocations (Slab Allocator)
    use alloc::boxed::Box;
    let b1 = Box::new(42u32);
    let b2 = Box::new(123u64);
    assert_eq!(*b1, 42);
    assert_eq!(*b2, 123);
    crate::serial_write("  -> Slab Cache (Small Objects) - PASSED\n");

    // 2. Test Large Allocations (Fallback / Linked List)
    let large = Box::new([0u8; 8192]);
    assert_eq!(large[0], 0);
    crate::serial_write("  -> Fallback (Large Blocks)    - PASSED\n");

    // 3. Test Dynamic growth
    use alloc::vec::Vec;
    let mut v = Vec::new();
    for i in 0..500 {
        v.push(i);
    }
    assert_eq!(v[499], 499);
    crate::serial_write("  -> Dynamic Vector Growth      - PASSED\n");

    crate::serial_write("[ SUCCESS ] All Allocator tests passed!\n");

    crate::vga_buffer::set_color(
        crate::vga_buffer::Color::White,
        crate::vga_buffer::Color::Black,
    );
}
