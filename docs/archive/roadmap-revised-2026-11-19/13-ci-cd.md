# CI/CD — Structured, Layered

## Purpose

CI must produce structured artifacts, not just "grep for passed."

## CI Levels

### PR Smoke (Fast, Every PR)

```text
cargo fmt --check
cargo clippy -- -D warnings
cargo build (default features)
cargo build (all features)
QEMU boot test (check selftests pass)
```

**Time budget:** < 5 minutes

### Integration (Every Merge to Main)

```text
All PR smoke checks
cargo test (unit tests)
QEMU userspace test (static binary runs)
QEMU fork/exec test
QEMU filesystem test (read/write files)
QEMU networking test (TCP/UDP)
```

**Time budget:** < 15 minutes

### Extended (Nightly)

```text
All integration checks
SMP stress test (8 CPUs)
Memory pressure test
Process churn test
I/O pressure test
FD exhaustion test
```

**Time budget:** < 30 minutes

### Nightly (Weekly)

```text
All extended checks
Fuzzing (24-hour run)
Crash injection tests
Long-running stress tests
```

**Time budget:** < 24 hours

## Artifacts

Every CI run produces:

```text
serial log         → full QEMU serial output
panic output       → kernel panics (if any)
kernel image       → bootable kernel binary
test report        → structured test results
coverage           → code coverage (if available)
reproducer         → crash reproduction steps (if applicable)
```

## CI Pipeline Design

```yaml
name: CI
on: [push, pull_request]
jobs:
  pr-smoke:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Format check
        run: cd kernel && cargo fmt --check
      - name: Clippy
        run: cd kernel && cargo clippy -- -D warnings
      - name: Build default
        run: cd kernel && cargo build
      - name: Build all features
        run: cd kernel && cargo build --no-default-features --features "smp,net,ext4,uhci,ash,hypervisor,verification"
      - name: QEMU boot test
        run: |
          cd kernel && cargo build --features self_test -Zbuild-std=core,alloc --target x86_64-unknown-none
          cd builder && cargo run --quiet
      - name: Upload artifacts
        uses: actions/upload-artifact@v3
        with:
          name: kernel-image
          path: kernel/target/x86_64-vahi/debug/bootimage-vahi_kernel.bin

  integration:
    needs: pr-smoke
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Unit tests
        run: cd kernel && cargo test
      - name: Userspace test
        run: |
          # Boot QEMU, run static binary, verify output
          timeout 30 qemu-system-x86_64 ... | grep "hello world"
      - name: Fork/exec test
        run: |
          # Boot QEMU, run fork/exec test, verify output
          timeout 30 qemu-system-x86_64 ... | grep "fork/exec passed"
```

## Acceptance Criteria

- [ ] PR smoke runs on every PR
- [ ] Integration runs on every merge
- [ ] Extended runs nightly
- [ ] Nightly runs weekly
- [ ] All artifacts uploaded
- [ ] Test report is structured

## Verification

1. **PR smoke:** Push PR, verify CI passes
2. **Integration:** Merge PR, verify integration passes
3. **Extended:** Check nightly run
4. **Nightly:** Check weekly run
