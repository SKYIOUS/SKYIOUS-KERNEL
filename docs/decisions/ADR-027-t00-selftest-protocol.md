# ADR-027: T-00 Kernel Self-Test Protocol and QEMU Exit Mechanism

## Status
Accepted (2026-09-15, phase T-00)

## Date
2026-09-15

## Context
T-00 requires that "did this specific behavior actually work?" be answerable
by automation. The kernel already had a substantial TAP-based selftest
framework (`kernel/src/selftest.rs`, ~135 registered tests) and a bash gate
(`tests/selftest_gate.sh`), but the loop had three structural gaps:

1. **No deterministic completion signal.** The kernel never exits QEMU: a
   finished suite looks identical to a hung boot, so runners relied on
   wall-clock timeouts even on success, and CI could not distinguish
   "passed" from "never finished".
2. **No panic result path.** The panic handler printed a human banner and
   halted; a panicking run had to be inferred from banner text, and the
   process never terminated on its own.
3. **No structured result classification on the host.** The bash gate
   greps for markers but has no timeout/boot-failure/unexpected-exit
   states, no build orchestration, and had silently diverged from the
   working boot path (it assumes a bootloader-crate image; the kernel
   boots via Limine).

## Decision

### 1. Protocol: keep TAP 13 on serial (extend, do not replace)
The existing TAP stream is retained verbatim — it already encodes
TEST_START (`[SELF-TEST] running test N/M: name`), TEST_PASS (`ok N - name`),
TEST_FAIL (`not ok N - name # reason`) and TEST_COMPLETE (`# P/T passed,
F failed`) in a standard, machine-parseable format. TEST_PANIC is added as
a TAP bail-out:

    [SELFTEST] T-00 test mode active     <- mode announce, first kernel output
    TAP version 13
    1..N
    [SELF-TEST] running test 1/N: name
    ok 1 - name                          | not ok 1 - name # reason
    ...
    # P/N passed, F failed               <- TEST_COMPLETE (summary)
    Bail out! KERNEL PANIC               <- TEST_PANIC (only on panic)

### 2. QEMU exit mechanism: isa-debug-exit at port 0xf4
The runner attaches `-device isa-debug-exit,iobase=0xf4,iosize=0x04`; the
kernel writes an exit code to port 0xf4, so QEMU terminates with
`(code & 0x7f) - 1`:

| Kernel constant (`selftest.rs`) | Port value | QEMU exit | Meaning |
|---|---|---|---|
| `QEMU_EXIT_PASS_BASE`  | 0x10 | 15 | suite completed, verdict from TAP |
| `QEMU_EXIT_PANIC_BASE` | 0x11 | 16 | panic handler fired |
| `QEMU_EXIT_TIMEOUT_BASE` (reserved) | 0x30 | 47 | kernel-side guard timeout |

Exit codes are a *second* signal only: the authoritative verdict is the TAP
summary parsed from serial. A run whose QEMU process exits is never treated
as success by itself.

### 3. Panic path
`panic_handler::handle_panic` (self_test builds only) first emits
`[PANIC] KERNEL PANIC` + `Bail out! KERNEL PANIC` on serial, then exits via
isa-debug-exit 0x11 and halts. Non-self_test builds keep the full
diagnostic banner + halt (unchanged behavior, no exit-device dependency).

### 4. Test-mode selection
The existing compile-time `self_test` Cargo feature remains the only
mechanism; production images (`default` features) contain no test code and
no banner. The four T-00 harness demos (`pass`/`fail`/`timeout`/`panic`)
are selected inside self_test builds via the `VAHI_SELFTEST_MODE`
compile-time env (`option_env!`); the default registers only the
deterministic-pass demo. No runtime fork of the kernel exists.

### 5. Host runner: `tests/run_qemu_tests.py` (Python, stdlib-only)
Replaces the bash gate as the canonical runner. Serial goes to a file
(`-serial file:...`) because QEMU-on-Windows stdio is not a byte pipe —
identical capture on Windows/Linux. Classification (exit codes):

| Code | Class | Condition |
|---|---|---|
| 0 | PASS | TAP summary present, `failed == 0`, `passed == total`, plan matches |
| 1 | FAIL | TAP summary present with failures (list included in diagnostics) |
| 2 | PANIC | panic marker without completed summary, or isa-exit 0x11 |
| 3 | TIMEOUT | no verdict within `--timeout` (process killed) |
| 4 | BOOT_FAILURE | QEMU died / no boot output before `--boot-wait` |
| 5 | UNEXPECTED_EXIT | QEMU exited mid-suite without verdict |
| 6 | INFRASTRUCTURE_ERROR | missing image/QEMU, runner I/O failure |

`--boot-only` provides the normal-boot regression check (PASS on reaching
`[BOOT] VFS init`; no TAP expected). `--qemu-extra` passes extra QEMU
arguments (the T-00 suite uses `-vga none`).

## Consequences
- CI can gate on real results (`ci.yml` `t00-harness` job) and demo
  failure/panic/timeout detection on demand (`.github/workflows/t00-demo.yml`).
- Timeout remains a failure, never a success; the wall clock is still the
  hang authority — the exit device covers the opposite failure mode
  (silent early death), where a timeout would waste the full budget.
- `tests/selftest_gate.sh` is superseded but retained for reference.
- Port 0xf4 writes are no-ops on real hardware and on aarch64; the kernel
  still halts after the write, so behavior without the device is a hang
  the runner classifies by timeout — unchanged from before T-00.

## Alternatives considered
- **`-serial stdio` + process exit parsing only:** rejected — unreliable on
  Windows, cannot distinguish hang from run-away success.
- **Kernel command line / initrd marker for test mode:** rejected — no
  existing bootinfo plumbing; compile-time feature already exists, is
  auditable in CI, and cannot leak into production images.
- **Custom `SKYIOUS_TEST:` protocol:** rejected — TAP already covers every
  required event, is standard, and 135 tests plus two gates already emit it.
