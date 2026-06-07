# Time System Calls

The time syscalls provide timing and sleep functionality.

## clock_gettime (syscall 93)

```c
int clock_gettime(clockid_t clockid, struct timespec *tp);
```

Retrieves the time from the specified clock.

**Clock IDs**:
| ID | Description |
|----|-------------|
| CLOCK_MONOTONIC | Time since boot (unaffected by NTP) |
| CLOCK_REALTIME | Wall-clock time |
| CLOCK_PROCESS_CPUTIME | CPU time consumed by this process |
| CLOCK_THREAD_CPUTIME | CPU time consumed by this thread |

## clock_settime (syscall 94)

```c
int clock_settime(clockid_t clockid, const struct timespec *tp);
```

Sets the time for the specified clock. Requires `CAP_SYS_TIME` capability.

## clock_getres (syscall 95)

```c
int clock_getres(clockid_t clockid, struct timespec *res);
```

Returns the resolution of the specified clock. The resolution is the minimum representable time difference.

## clock_nanosleep (syscall 96)

```c
int clock_nanosleep(clockid_t clockid, int flags, const struct timespec *request, struct timespec *remain);
```

Suspends the calling thread until the specified time has elapsed.

**Flags**:
- `0`: Sleep for the duration specified by `request`
- `TIMER_ABSTIME`: Sleep until absolute time specified by `request`

## nanosleep (syscall 35)

```c
int nanosleep(const struct timespec *req, struct timespec *rem);
```

High-resolution sleep with microsecond granularity. The `rem` parameter returns the remaining time if the sleep was interrupted.

## Timer Syscalls (97-100)

```c
int timer_create(clockid_t clockid, struct sigevent *sevp, timer_t *timerid);
int timer_settime(timer_t timerid, int flags, const struct itimerspec *new_value, struct itimerspec *old_value);
int timer_gettime(timer_t timerid, struct itimerspec *curr_value);
int timer_delete(timer_t timerid);
```

POSIX timer management. Timers can deliver signals or notification on expiration.

## alarm (syscall 38)

```c
unsigned int alarm(unsigned int seconds);
```

Sets a simple alarm that delivers `SIGALRM` after the specified number of seconds.

## getitimer / setitimer (syscalls 36-37)

```c
int getitimer(int which, struct itimerval *curr_value);
int setitimer(int which, const struct itimerval *new_value, struct itimerval *old_value);
```

Interval timer management with microsecond resolution.
