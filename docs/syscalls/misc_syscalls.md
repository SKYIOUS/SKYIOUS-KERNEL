# Miscellaneous System Calls

These syscalls don't fit neatly into other categories.

## uname (syscall 49)

```c
int uname(struct utsname *buf);
```

Returns system identification information including OS name, hostname, kernel release, kernel version, machine architecture, and domain name. All fields are null-terminated strings.

## sysinfo (syscall 50)

```c
int sysinfo(struct sysinfo *info);
```

Returns overall system statistics: total/available RAM, swap usage, process count, load averages, and uptime.

## getrusage (syscall 51)

```c
int getrusage(int who, struct rusage *usage);
```

Returns resource usage statistics. `who` can be `RUSAGE_SELF`, `RUSAGE_CHILDREN`, or `RUSAGE_THREAD`. Fields include user/kernel CPU time, page faults, context switches, and I/O counts.

## getcwd / chdir / fchdir (syscalls 52-54)

```c
char *getcwd(char *buf, size_t size);
int chdir(const char *path);
int fchdir(int fd);
```

Manages the current working directory. `getcwd` returns the absolute path of the CWD. `chdir` changes the CWD to the specified path. `fchdir` changes it using a directory file descriptor.

## access (syscall 21)

```c
int access(const char *pathname, int mode);
```

Checks whether the calling process can access the file. `mode` is a bitmask of `R_OK`, `W_OK`, `X_OK`, and `F_OK`.

## sched_yield (syscall 24)

```c
int sched_yield(void);
```

Voluntarily yields the CPU to allow other tasks to run. Returns 0 on success.

## pause (syscall 34)

```c
int pause(void);
```

Suspends the calling process until a signal is received. Always returns -1 with `EINTR`.

## sigaction / sigprocmask (syscalls 13-15)

```c
int rt_sigaction(int signum, const struct sigaction *act, struct sigaction *oldact);
int rt_sigprocmask(int how, const sigset_t *set, sigset_t *oldset);
int rt_sigreturn(void);
```

Signal handling operations.

## futex (syscall 233)

```c
int futex(int *uaddr, int futex_op, int val, const struct timespec *timeout, int *uaddr2, int val3);
```

Fast userspace mutex operation. Used for implementing efficient userspace synchronization.

## set_tid_address (syscall 232)

```c
void *set_tid_address(int *tidptr);
```

Sets the pointer to the thread ID for thread-local storage.
