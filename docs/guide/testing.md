# How to Write and Run Tests

SkyOS has a multi-layered testing strategy covering unit tests, integration tests, and in-QEMU tests.

## Unit Tests

Standard Rust unit tests validate kernel components in isolation:

```bash
cargo test --lib
```

Write unit tests in a `#[cfg(test)]` module at the bottom of each source file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn test_slab_allocation() {
        let mut alloc = SlabAllocator::new(64);
        let ptr = alloc.allocate().unwrap();
        assert!(!ptr.is_null());
        alloc.deallocate(ptr);
    }
}
```

## Integration Tests

Integration tests run inside QEMU and validate full system behavior:

```bash
cargo test --test integration
```

These tests can:
- Write to serial output for verification
- Trigger syscalls and check results
- Verify memory mappings through page table inspection

## Kernel Test Framework

The `kernel_test!` macro defines tests that run in kernel context:

```rust
kernel_test!(test_memory_map, {
    let map = memory_map();
    assert!(map.total_pages > 0);
});
```

## Running in CI

Tests are automatically run in CI on every pull request. The CI pipeline includes:
- `cargo check` for compilation errors
- `cargo test --lib` for unit tests
- `cargo test --test integration` for QEMU tests
- `cargo clippy` for linting
