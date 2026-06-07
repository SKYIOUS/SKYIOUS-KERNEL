# Complete Syscall Table

This page lists all system calls implemented in SkyOS, organized by number.

## Syscall Convention

- RAX: syscall number
- RDI, RSI, RDX, R10, R8, R9: arguments
- Return value in RAX (negative = errno on error)

## Syscall Table

| # | Name | Description | Status |
|---|------|-------------|--------|
| 0 | read | Read from file descriptor | Done |
| 1 | write | Write to file descriptor | Done |
| 2 | open | Open a file | Done |
| 3 | close | Close a file descriptor | Done |
| 4 | stat | Get file status | Done |
| 5 | fstat | Get file descriptor status | Done |
| 6 | lstat | Get symbolic link status | Done |
| 7 | poll | Wait for I/O events | Done |
| 8 | lseek | Reposition file offset | Done |
| 9 | mmap | Map memory | Done |
| 10 | mprotect | Set memory protection | Done |
| 11 | munmap | Unmap memory | Done |
| 12 | brk | Change data segment size | Done |
| 13 | rt_sigaction | Set signal handler | Done |
| 14 | rt_sigprocmask | Examine/change signal mask | Done |
| 15 | rt_sigreturn | Return from signal handler | Done |
| 16 | ioctl | Device control | Done |
| 17 | pread64 | Positional read | Done |
| 18 | pwrite64 | Positional write | Done |
| 19 | readv | Scatter-gather read | Done |
| 20 | writev | Scatter-gather write | Done |
| 21 | access | Check file permissions | Done |
| 22 | pipe | Create pipe | Done |
| 23 | select | Synchronous I/O multiplexing | Done |
| 24 | sched_yield | Yield processor | Done |
| 25 | mremap | Remap virtual memory | Done |
| 26 | msync | Synchronize mapped memory | Done |
| 27 | mincore | Determine page residency | Done |
| 28 | madvise | Give memory advice | Done |
| 29 | shmget | Allocate shared memory | Done |
| 30 | shmat | Attach shared memory | Done |
| 31 | shmdt | Detach shared memory | Done |
| 32 | dup | Duplicate file descriptor | Done |
| 33 | dup2 | Duplicate to specific fd | Done |
| 34 | pause | Wait for signal | Done |
| 35 | nanosleep | High-resolution sleep | Done |
| 36 | getitimer | Get interval timer | Done |
| 37 | setitimer | Set interval timer | Done |
| 38 | alarm | Schedule alarm | Done |
| 39 | getpid | Get process ID | Done |
| 40 | getppid | Get parent process ID | Done |
| 41 | getuid | Get user ID | Done |
| 42 | geteuid | Get effective user ID | Done |
| 43 | getgid | Get group ID | Done |
| 44 | getegid | Get effective group ID | Done |
| 45 | setuid | Set user ID | Done |
| 46 | setgid | Set group ID | Done |
| 47 | getgroups | Get supplementary group IDs | Done |
| 48 | setgroups | Set supplementary group IDs | Done |
| 49 | uname | Get system information | Done |
| 50 | sysinfo | Get system statistics | Done |
| 51 | getrusage | Get resource usage | Done |
| 52 | getcwd | Get current working directory | Done |
| 53 | chdir | Change directory | Done |
| 54 | fchdir | Change directory by fd | Done |
| 55 | mkdir | Create directory | Done |
| 56 | rmdir | Remove directory | Done |
| 57 | fork | Create child process | Done |
| 58 | vfork | Create child and block parent | Done |
| 59 | execve | Execute program | Done |
| 60 | exit | Terminate process | Done |
| 61 | wait4 | Wait for process | Done |
| 62 | kill | Send signal | Done |
| 63 | unlink | Remove file | Done |
| 64 | link | Create hard link | Done |
| 65 | symlink | Create symbolic link | Done |
| 66 | readlink | Read symbolic link target | Done |
| 67 | chmod | Change file mode | Done |
| 68 | chown | Change file owner | Done |
| 69 | utimes | Change file timestamps | Done |
| 70 | rename | Rename file | Done |
| 71 | truncate | Truncate file | Done |
| 72 | ftruncate | Truncate file by fd | Done |
| 73 | fsync | Sync file to disk | Done |
| 74 | fdatasync | Sync data only | Done |
| 75 | mount | Mount filesystem | Done |
| 76 | umount2 | Unmount filesystem | Done |
| 77 | getdents | Get directory entries | Done |
| 78 | getdents64 | Get directory entries (64-bit) | Done |
| 79 | socket | Create socket | Planned |
| 80 | connect | Connect socket | Planned |
| 81 | bind | Bind socket to address | Planned |
| 82 | listen | Listen on socket | Planned |
| 83 | accept | Accept connection | Planned |
| 84 | sendto | Send message on socket | Planned |
| 85 | recvfrom | Receive message from socket | Planned |
| 86 | sendmsg | Send message with ancillary data | Planned |
| 87 | recvmsg | Receive message with ancillary data | Planned |
| 88 | shutdown | Shut down socket | Planned |
| 89 | setsockopt | Set socket options | Planned |
| 90 | getsockopt | Get socket options | Planned |
| 91 | getsockname | Get socket name | Planned |
| 92 | getpeername | Get peer name | Planned |
| 93 | clock_gettime | Get clock time | Done |
| 94 | clock_settime | Set clock time | Done |
| 95 | clock_getres | Get clock resolution | Done |
| 96 | clock_nanosleep | Sleep with clock | Done |
| 97 | timer_create | Create timer | Done |
| 98 | timer_settime | Set timer | Done |
| 99 | timer_gettime | Get timer | Done |
| 100 | timer_delete | Delete timer | Done |
| 231 | exit_group | Exit all threads | Done |
| 232 | set_tid_address | Set TID address | Done |
| 233 | futex | Fast userspace mutex | Done |
| 300 | skyos_create_window | Create GUI window | Done |
| 301 | skyos_get_buffer | Get window buffer | Done |
| 302 | skyos_flush | Flush window updates | Done |
| 303 | skyos_map_buffer | Map GPU buffer | Done |
| 304 | skyos_get_display_info | Get display info | Done |
| 305 | skyos_set_cursor | Set cursor position | Done |
| 306 | skyos_event_wait | Wait for input event | Done |
