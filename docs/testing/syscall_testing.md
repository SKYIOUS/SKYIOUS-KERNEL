# Syscall Test Harness

The syscall test suite validates every system call against expected behavior.

## Test Harness

The syscall test harness runs in userspace and calls syscalls directly:

```rust
pub fn test_open_read_close() -> Result<(), &'static str> {
    let fd = sys_open("/test/file.txt", O_RDONLY, 0);
    if fd < 0 { return Err("open failed"); }

    let mut buf = [0u8; 1024];
    let n = sys_read(fd, &mut buf, 1024);
    if n < 0 { return Err("read failed"); }

    let ret = sys_close(fd);
    if ret < 0 { return Err("close failed"); }

    Ok(())
}
```

## Test Categories

1. **Happy path**: Syscalls with valid arguments should succeed
2. **Error path**: Syscalls with invalid arguments should return correct errno
3. **Edge cases**: Boundary values (zero length, maximum size, NULL pointers)
4. **Resource limits**: File descriptor exhaustion, memory limits
5. **Concurrency**: Multiple threads calling syscalls simultaneously
6. **Signal interaction**: Syscalls interrupted by signals

## Automated Testing

Each test is defined as a kernel test case:

```rust
kernel_test_suite!(syscall_open,
    test_open_existing,
    test_open_nonexistent_enoent,
    test_open_create,
    test_open_exclusive_eexist,
    test_open_directory_enotdir,
    test_open_permission_denied_eacces,
);
```

## Running

```bash
# Run all syscall tests
cargo test --test integration syscall

# Run specific syscall tests
cargo test --test integration syscall_open

# Run with detailed output
cargo test --test integration syscall -- --show-output
```
