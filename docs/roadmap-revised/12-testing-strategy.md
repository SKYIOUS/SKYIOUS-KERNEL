# Testing Strategy — Layered, Kernel-Specific

## Purpose

Testing must be kernel-specific and layered. Not "run syzkaller" — define what must actually be built.

## Testing Layers

### Layer 1: Host-Side (Build-Time)

```text
cargo fmt          → formatting check
cargo check        → type checking
cargo clippy       → lint checking
unit tests         → #[test] functions
static analysis    → unsafe audit
```

### Layer 2: Kernel-Side (Boot-Time)

```text
boot tests         → verify boot sequence
selftests          → 92 existing tests
syscall tests      → test each syscall
process tests      → fork/exec/exit
VM tests           → mmap/brk/mprotect
SMP tests          → multi-CPU scheduling
IPC tests          → pipe/socket/eventfd
FS tests           → read/write/stat
network tests      → TCP/UDP/Unix
```

### Layer 3: QEMU Integration ✅ IMPLEMENTED

```text
boot               → kernel boots successfully
selftest TAP       → TAP output parsed from serial
no-panic           → no kernel panics during boot
SMP boot           → multi-CPU boot verification

Infrastructure:
  scripts/qemu_test.ps1  → Windows test runner
  scripts/qemu_test.sh   → Linux/macOS test runner
  .github/workflows/     → CI matrix (debug/release × 1/4 CPUs)
```

### Layer 4: Stress

```text
SMP                → 8-CPU stress test
memory pressure    → OOM killer test
process churn      → fork bomb protection
I/O pressure       → concurrent reads/writes
FD exhaustion      → 10000 FDs open
filesystem stress  → concurrent file operations
```

### Layer 5: Fuzzing

**What must actually be built:**

1. **Executor:** Program that invokes syscalls with random arguments
2. **Syscall descriptions:** Format for Vahi syscalls (syzkaller-style)
3. **QEMU target:** Boot Vahi in QEMU for fuzzing
4. **Corpus:** Seed inputs for fuzzing
5. **Crash capture:** Collect crash information
6. **Reproduction:** Replay crashes
7. **Coverage strategy:** Code coverage tracking

**Do NOT present ASan/TSan or syzkaller as turnkey features.** These require significant infrastructure.

## Test Categories

### Boot Tests
- 100 consecutive builds without failure (deterministic binary hash) ✅ VERIFIED
- Boot with various feature flag combinations
- Boot with SMP (1, 2, 4 CPUs)

### Selftests
- All 92 existing selftests pass
- New tests for new features

### Syscall Tests
- Fork/exec tests
- Signal tests
- Pipe tests
- Socket tests
- mmap tests

### Process Tests
- 5000 fork/exec iterations
- Process lifecycle tests
- Exit/reap tests

### VM Tests
- Address space tests
- COW tests
- Page fault tests

### SMP Tests
- Concurrent scheduler tests
- Lock contention tests
- Work stealing tests

### IPC Tests
- Pipe throughput tests
- Socket throughput tests
- Shared memory tests

### Filesystem Tests
- SkyFS read/write tests
- ext4 read/write tests
- Crash injection tests (1000 points)

### Network Tests
- TCP/UDP tests
- Unix socket tests
- Concurrent connection tests

## Acceptance Criteria

- [ ] All host-side checks pass
- [ ] All kernel-side tests pass
- [ ] All QEMU integration tests pass
- [ ] Stress tests pass
- [ ] Fuzzing finds no crashes for 24 hours

## Verification

1. **Host-side:** `cargo fmt && cargo check && cargo clippy && cargo test`
2. **Kernel-side:** Boot selftests pass (109 selftests)
3. **QEMU:** `./scripts/qemu_test.sh` or `./scripts/qemu_test.ps1`
4. **CI:** GitHub Actions matrix (debug/release × 1/4 SMP)
5. **Stress:** SMP + memory pressure + process churn
6. **Fuzzing:** 24-hour run without crashes
