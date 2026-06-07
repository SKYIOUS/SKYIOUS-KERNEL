# Timekeeping and Timer Subsystem

SkyOS maintains monotonic and wall-clock time through a combination of hardware timers and software timekeeping.

## Time Sources

The kernel uses the following hardware timers:
- **HPET** (High Precision Event Timer): Primary time source with sub-microsecond resolution
- **TSC** (Time Stamp Counter): Cycle-accurate timing for short measurements
- **PIT** (Programmable Interval Timer): Legacy fallback on systems without HPET
- **RTC** (Real-Time Clock): Battery-backed wall-clock time

## Timer Management

The kernel maintains a global timer wheel with timer granularity of 1 microsecond. Timers are organized into a hierarchical timing wheel for efficient insertion and expiration:

```rust
pub struct TimerWheel {
    slots: Vec<Vec<TimerEntry>>,
    current_tick: u64,
    granularity_ns: u64,
}
```

## Clock IDs

The kernel exposes these clocks via `clock_gettime()`:
- `CLOCK_MONOTONIC`: Time since boot, unaffected by NTP adjustments
- `CLOCK_REALTIME`: Wall-clock time, settable by privileged processes
- `CLOCK_THREAD_CPUTIME`: Per-thread CPU time
- `CLOCK_PROCESS_CPUTIME`: Per-process CPU time

## Sleep and Timeouts

The `nanosleep()` syscall suspends the calling task until the specified duration elapses. Internally, it registers a timer callback that wakes the task. Timeouts on IPC and I/O operations are implemented using the same timer infrastructure.

## NTP Support

Clock adjustment parameters are managed through `adjtimex()`, allowing NTP daemons to slew the clock frequency without abrupt jumps.
