# Unit Testing Approach

Unit tests validate individual functions and modules in isolation.

## Kernel Unit Tests

The kernel uses Rust's built-in test framework with a custom test runner that can execute tests in kernel context:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn test_slab_alloc_free() {
        let mut slab = SlabAllocator::new(64, 1024);
        let ptr = slab.allocate().unwrap();
        assert!(!ptr.is_null());
        slab.deallocate(ptr);
        // Verify the block was returned to the free list
        assert_eq!(slab.free_count(), 1024);
    }

    #[test_case]
    fn test_buddy_alloc_alignment() {
        let mut buddy = BuddyAllocator::new(1 << 20);
        let block = buddy.allocate(4096).unwrap();
        assert_eq!(block.addr() % 4096, 0);
    }
}
```

## Test Organization

Tests are organized by module:
- `src/memory/tests.rs` - Memory allocator tests
- `src/scheduler/tests.rs` - Scheduler tests
- `src/vfs/tests.rs` - VFS layer tests
- `src/sync/tests.rs` - Synchronization tests

## Test Dependencies

Unit tests avoid hardware dependencies. Hardware-backed functions are replaced with mock implementations during testing. The `#[cfg(test)]` attribute ensures test code is excluded from release builds.

## Running Unit Tests

```bash
# Run all unit tests
cargo test --lib

# Run specific module tests
cargo test --lib memory::tests

# Run with output
cargo test --lib -- --nocapture
```
