# Process System Calls

The process syscalls manage process creation, execution, and lifecycle.

## fork (syscall 57)

```c
pid_t fork(void);
```

Creates a child process that is a copy of the parent. The child gets a copy of the parent's address space (copy-on-write), file descriptors, and signal handlers. Returns the child's PID to the parent, 0 to the child, or -1 on error.

## vfork (syscall 58)

```c
pid_t vfork(void);
```

Creates a child process that blocks the parent until the child calls `execve()` or `exit()`. The child shares the parent's address space. More efficient than `fork()` when followed by `execve()`.

## execve (syscall 59)

```c
int execve(const char *pathname, char *const argv[], char *const envp[]);
```

Replaces the current process image with a new program. The calling process's code, data, heap, and stack are replaced. PID, file descriptors (with `FD_CLOEXEC` handling), and signal dispositions are preserved.

## exit (syscall 60)

```c
void exit(int status);
```

Terminates the calling process. Low 8 bits of `status` are reported to the parent via `wait()`. All resources are freed, file descriptors are closed, and the process enters `ZOMBIE` state until the parent calls `wait()`.

## exit_group (syscall 231)

```c
void exit_group(int status);
```

Terminates all threads in the calling process group. Equivalent to `exit()` for single-threaded processes.

## wait4 (syscall 61)

```c
pid_t wait4(pid_t pid, int *wstatus, int options, struct rusage *rusage);
```

Waits for a child process to change state. By default, waits for any child to terminate. `WNOHANG` returns immediately if no child has exited.

## clone

```c
int clone(int (*fn)(void *), void *stack, int flags, void *arg, ...);
```

Creates a new thread. The `flags` parameter controls which resources are shared with the parent (address space, file descriptors, signal handlers, etc.).

## getpid / getppid (syscalls 39-40)

```c
pid_t getpid(void);
pid_t getppid(void);
```

Returns the process ID and parent process ID, respectively.
