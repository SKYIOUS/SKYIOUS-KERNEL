# Memory Allocator Tests

The memory subsystem has dedicated test suites for each allocator component.

## Page Frame Allocator Tests

```rust
#[test_case]
fn test_frame_alloc_exhaustion() {
    // Allocate all frames, then verify allocation fails
    let mut allocated = Vec::new();
    while let Some(frame) = frame_alloc::allocate() {
        allocated.push(frame);
    }
    assert!(frame_alloc::allocate().is_none());

    // Free all frames, verify allocation works again
    for frame in allocated.drain(..) {
        frame_alloc::deallocate(frame);
    }
    assert!(frame_alloc::allocate().is_some());
}
```

## Slab Allocator Tests

- Single allocation and deallocation
- Interleaved allocation patterns
- Maximum capacity stress test
- Alignment verification for all sizes (8, 16, 32, 64, ..., 2048)
- Concurrent allocation from multiple threads

## Buddy Allocator Tests

- Power-of-two size allocation
- Splitting and coalescing verification
- Maximum allocatable block (entire heap)
- Large fragmentation test

## Virtual Memory Tests

- Page table creation and mapping
- Page protection changes (read/write/execute/none)
- Large mapping (huge pages)
- Mapping cleanup on process exit
- Guard page protection

## Memory Leak Detection

The test framework tracks all allocations and verifies that:
- All allocated memory is freed during cleanup
- No double-frees occur
- Free list integrity is maintained
- Reference counts for shared mappings are correct

## Running

```bash
cargo test --lib memory
```
