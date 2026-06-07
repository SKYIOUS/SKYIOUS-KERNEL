# System Call Reference

SkyOS implements POSIX-compatible system calls alongside kernel-specific extensions for GUI and IPC operations.

## Syscall Convention

All syscalls follow the standard calling convention:
- RAX: syscall number
- RDI, RSI, RDX, R10, R8, R9: arguments 1-6
- Return value in RAX (negative = error)

## Syscall Table

| Number | Name | Description |
|--------|------|-------------|
| 0 | read | Read from file descriptor |
| 1 | write | Write to file descriptor |
| 2 | open | Open a file |
| 3 | close | Close a file descriptor |
| 4 | stat | Get file status |
| 5 | fstat | Get file descriptor status |
| 9 | mmap | Map memory |
| 10 | munmap | Unmap memory |
| 12 | brk | Change data segment size |
| 57 | fork | Create a child process |
| 59 | execve | Execute a program |
| 60 | exit | Terminate the calling process |
| 61 | wait4 | Wait for process termination |
| 62 | kill | Send a signal |
| 78 | getdents | List directory entries |
| 231 | exit_group | Exit all threads in a process |
| 300 | skyos_create_window | Create a GUI window |
| 301 | skyos_get_buffer | Get window framebuffer |
| 302 | skyos_flush | Flush window updates |
| 303 | skyos_map_buffer | Map GPU buffer |

## Return Values

All syscalls return `i64`. On success, values >= 0. On failure, values < 0 with the errno encoded as `-errno`. Common error codes: `-EINVAL`, `-ENOMEM`, `-EACCES`, `-ENOENT`, `-EBADF`, `-EIO`.

## Thread Safety

Syscalls are reentrant and can be called from any thread. The kernel handles synchronization internally.
