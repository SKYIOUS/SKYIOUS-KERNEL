# ADR-011: Single Kernel Clock

## Status
Proposed

## Date
2026-08-20

## Context
The review found multiple time sources: `interrupts::get_ticks()` (PIT/APIC tick counter), `drivers/rtc` (RTC read at boot), `syscalls/posix_timers.rs` (its own tick handling), scheduler quantum counts, and `boot::state` boot timestamps. Multiple clocks drift apart (tick counter vs RTC vs timerfd), which makes `clock_gettime`, timeout math, and uptime reporting inconsistent.

## Decision
One kernel tick source: `interrupts::get_ticks()` becomes THE monotonic clock. All other time derivations read from it:

- RTC: read once at boot for wall-clock offset; `clock_gettime(CLOCK_REALTIME)` = boot_rtc + ticks_to_ns (offset arithmetic, no repeated port reads)
- `posix_timers.rs`: reuse tick-derived time, drop its private counter
- Scheduler quanta: count in ticks (already does), no separate clock
- `clock_gettime(CLOCK_MONOTONIC)` returns ticks directly

`ticks_to_ns` uses the PIT frequency as the single conversion constant (no per-device calibration table yet).

## Alternatives Considered

### TSC as the clock
- Pros: fine-grained, fast
- Cons: SMP TSC drift/desync handling, frequency calibration on real hardware; overkill when tick granularity suffices
- Rejected now; revisit if a userland benchmark needs sub-ms timing (perf work)

### Per-device time (status quo)
- Pros: none
- Cons: divergent clocks — the bug class this ADR removes
- Rejected

## Consequences
- One constant (tick frequency) converts ticks→ns; drift between RTC and ticks bounded by tick accuracy
- Deletes RTC port reads from the hot path (RTC read once at boot)
- Timerfd/posix timers become a pure function of the tick count
- `uptime`/`sysinfo`/`time` all agree
- ponytail: single conversion constant, no calibration table — add per-device calibration only if real hardware drift is measured (hardware clock is never ideal on paper)