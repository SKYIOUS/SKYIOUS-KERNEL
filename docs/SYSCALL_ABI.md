# SkyOS Syscall ABI

This document specifies the Application Binary Interface (ABI) for making system calls to the Skyious kernel.

## 1. Invocation

- **Instruction:** `syscall`
- **Register Usage:**
  - `rax`: Syscall number
  - `rdi`: Argument 1
  - `rsi`: Argument 2
  - `rdx`: Argument 3
  - `r10`: Argument 4
  - `r8`:  Argument 5
  - `r9`:  Argument 6
- **Return Value:** The result of the syscall is returned in `rax`.
- **Clobbered Registers:** `rcx` and `r11` are clobbered by the `syscall`/`sysret` instructions and cannot be used to pass arguments.

## 2. Return Values and Errors

- **Success:** A non-negative value in `rax` indicates success.
- **Error:** A negative value in `rax` indicates an error. The value is the negation of one of the `errno` constants (e.g., `-ENOENT`).

## 3. Syscall Table

| Number | Name          | `arg1` (rdi) | `arg2` (rsi)  | `arg3` (rdx)   | `arg4` (r10) | `arg5` (r8) | `arg6` (r9) | Description |
|--------|---------------|--------------|---------------|----------------|--------------|-------------|-------------|-------------|
| 0      | `sys_read`    | `fd`         | `buf: *mut u8`| `count`        |              |             |             | Read from a file descriptor. |
| 1      | `sys_write`   | `fd`         | `buf: *const u8` | `count`     |              |             |             | Write to a file descriptor. |
| 2      | `sys_open`    | `path: *const u8` | `flags`    |                |              |             |             | Open a file. |
| 3      | `sys_close`   | `fd`         |               |                |              |             |             | Close a file descriptor. |
| 4      | `sys_stat`    | `path: *const u8` | `statbuf: *mut Stat` |      |              |             |             | Get file status. |
| 5      | `sys_fstat`   | `fd`         | `statbuf: *mut Stat` |           |              |             |             | Get file status of an open file. |
| 6      | `sys_lseek`   | `fd`         | `offset`      | `whence`       |              |             |             | Reposition file offset. |
| 9      | `sys_mmap`    | `addr`       | `len`         | `prot`         | `flags`      | `fd`        | `offset`    | Map files or devices into memory. |
| 11     | `sys_munmap`  | `addr`       | `len`         |                |              |             |             | Unmap files or devices from memory. |
| 12     | `sys_brk`     | `addr`       |               |                |              |             |             | Set the program break. |
| 15     | `sys_rt_sigreturn` | `regs_ptr` (implicit) | |             |              |             |             | Return from signal handler. |
| 22     | `sys_pipe`    | `fds: *mut u32` |             |                |              |             |             | Create a pipe. |
| 33     | `sys_dup2`    | `oldfd`      | `newfd`       |                |              |             |             | Duplicate a file descriptor. |
| 35     | `sys_nanosleep`| `req: *const timespec` | `rem: *mut timespec` | |            |             |             | Pause execution. |
| 39     | `sys_getpid`  |              |               |                |              |             |             | Get process ID. |
| 57     | `sys_fork`    |              |               |                |              |             |             | Create a child process. |
| 59     | `sys_execve`  | `path: *const u8` | `argv: *const *const u8` | `envp: *const *const u8` | |         |             | Execute a program. |
| 61     | `sys_wait4`   | `pid`        | `status: *mut i32` | `options` | `rusage: *mut rusage` |       |             | Wait for process to change state. |
| 62     | `sys_rt_sigaction` | `sig`   | `act: *const u64` | `oldact: *mut u64` | |          |             | Set signal action handler. |
| 63     | `sys_uname`   | `buf: *mut UtsName` |        |                |              |             |             | Get system information. |
| 79     | `sys_getcwd`  | `buf: *mut u8` | `size`     |                |              |             |             | Get current working directory. |
| 80     | `sys_chdir`   | `path: *const u8` |           |                |              |             |             | Change working directory. |
| 83     | `sys_mkdir`   | `path: *const u8` | `mode`     |                |              |             |             | Create a directory. |
| 87     | `sys_unlink`  | `path: *const u8` |           |                |              |             |             | Delete a name and possibly the file it refers to. |
| 110    | `sys_getppid` |              |               |                |              |             |             | Get parent process ID. |
