# Regression Test Suite

The regression test suite ensures that fixed bugs stay fixed and working features continue to work.

## Regression Test Database

Each regression test is linked to a GitHub issue:

```rust
// Issue #42: mmap with MAP_FIXED overwrites existing mappings
#[test_case]
fn test_regression_42_mmap_map_fixed() {
    let addr1 = sys_mmap(0, 4096, PROT_READ, MAP_PRIVATE | MAP_ANONYMOUS, 0, 0);
    assert!(addr1 > 0);
    let addr2 = sys_mmap(addr1 as usize, 4096, PROT_READ,
        MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, 0, 0);
    assert_eq!(addr2, addr1);
    // Verify the old mapping was replaced
    sys_munmap(addr2 as usize, 4096);
}
```

## Regression Test Organization

Tests are organized by subsystem:
- `tests/regression/memory.rs` - Memory management regressions
- `tests/regression/fs.rs` - Filesystem regressions
- `tests/regression/sched.rs` - Scheduler regressions
- `tests/regression/ipc.rs` - IPC regressions
- `tests/regression/syscall.rs` - Syscall regressions

## Adding a Regression Test

When a bug is fixed:
1. Write a test that reproduces the original bug
2. Verify the test fails without the fix
3. Add the test to the regression suite
4. Reference the issue number in the test name and comment

## Running Regression Tests

```bash
# Run all regression tests
cargo test --test regression

# Run specific regression test
cargo test regression_42

# Run with verbose output
cargo test --test regression -- --nocapture
```

## Automated Regression Detection

The CI pipeline:
- Compares test results against the previous build
- Flags newly failing tests for immediate attention
- Generates a regression report with each build
- Blocks merges if regression tests fail
