# Integration Testing in QEMU

Integration tests run the full kernel inside QEMU and validate system-level behavior.

## Test Framework

Integration tests use a custom runner that:
1. Builds the kernel with test configuration
2. Boots it in QEMU
3. Monitors serial output for test results
4. Reports pass/fail based on serial output patterns

## Writing Integration Tests

Tests are Rust source files in `tests/`:

```rust
// tests/vm_test.rs
#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(skyos_test::runner)]

use skyos_test::*;

#[test_case]
fn test_page_fault_recovery() {
    let result = unsafe { map_and_access_page(0x1000_0000_0000) };
    assert!(result.is_err());
}
```

## What Integration Tests Cover

- **Syscall correctness**: All syscalls are tested with valid and invalid arguments
- **Process isolation**: Verify that processes cannot access each other's memory
- **Memory management**: Stress test allocation and deallocation
- **Scheduler**: Create many tasks and verify they all make progress
- **IPC**: Message passing, shared memory, and signaling

## Running Integration Tests

```bash
# Run all integration tests
cargo test --test integration

# Run a specific integration test
cargo test --test integration test_vm

# Run with QEMU display (for visual debugging)
QEMU_DISPLAY=true cargo test --test integration
```

## Test Configuration

Integration tests use a special kernel config that enables all debugging features and sets the log level to trace. The QEMU configuration is controlled by environment variables:
- `QEMU_MEM` (default: 256M)
- `QEMU_SMP` (default: 1)
- `QEMU_DISPLAY` (default: none)
