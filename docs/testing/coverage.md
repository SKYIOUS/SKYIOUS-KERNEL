# Code Coverage Tracking

SkyOS uses LLVM-based code coverage to track test effectiveness.

## Coverage Tools

The kernel uses `cargo test` with LLVM coverage instrumentation:

```bash
# Generate coverage data
CARGO_INCREMENTAL=0 RUSTFLAGS="-Cinstrument-coverage" \
    LLVM_PROFILE_FILE="target/coverage/%p-%m.profraw" \
    cargo test

# Generate coverage report
grcov . -s . --binary-path ./target/debug/ -t html \
    --branch --ignore-not-existing -o target/coverage/html
```

## Coverage Targets

| Area | Target Coverage | Current |
|------|----------------|---------|
| Memory management | 95% | 91% |
| Scheduler | 90% | 85% |
| Syscall dispatch | 100% | 100% |
| Individual syscalls | 90% | 82% |
| VFS layer | 85% | 78% |
| Drivers | 70% | 55% |
| IPC | 90% | 87% |
| Error handling paths | 80% | 72% |

## Coverage Gaps

Known coverage gaps are tracked:
- Error paths in drivers (hardware failure simulation is difficult)
- Race conditions in concurrent code
- Boot-time initialization sequences
- ACPI table parsing edge cases

## Improving Coverage

To improve coverage:
- Add tests for error handling paths
- Write fuzz tests for syscall argument validation
- Test with different hardware configurations
- Add stress tests for concurrent operations

## Coverage in CI

Coverage is reported in CI for every pull request. Changes that significantly reduce coverage require justification. The coverage report is uploaded as a build artifact and linked in the PR checks.
