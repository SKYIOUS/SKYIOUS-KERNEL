# Testing Strategy Overview

SkyOS uses a multi-layered testing strategy to ensure correctness and reliability.

## Test Layers

| Layer | Tool | Scope | Speed |
|-------|------|-------|-------|
| Unit tests | `cargo test` | Individual functions | Milliseconds |
| Integration tests | `cargo test --test` | Subsystem interaction | Seconds |
| Kernel tests | QEMU runner | Full kernel in VM | Minutes |
| Stress tests | Custom scripts | Long-running stability | Hours |

## Testing Philosophy

1. **Test at the lowest level possible**: Unit tests catch bugs early and run fast
2. **Test error paths thoroughly**: Every syscall, allocation, and I/O operation should be tested for failure modes
3. **Automate everything**: All tests run in CI on every pull request
4. **Reproducible**: Tests should produce the same results every time
5. **Fast feedback**: Unit tests complete in seconds; developers run them before every commit

## Test Infrastructure

- **Test runner**: Custom test harness that runs inside the kernel context
- **Serial output verification**: Tests print results to serial port; the harness validates output
- **QEMU automation**: Tests boot the kernel in QEMU and check exit codes or serial patterns
- **Coverage tracking**: LLVM coverage tools measure code coverage

## Running Tests

```bash
# Unit tests
cargo test

# Integration tests (in QEMU)
cargo test --test integration

# Specific test
cargo test test_name
```
