# read() Syscall

Read data from a file descriptor.

## Synopsis

```c
ssize_t read(int fd, void *buf, size_t count);
```

## Arguments

| Argument | Type | Description |
|----------|------|-------------|
| fd | int | File descriptor to read from |
| buf | void* | Buffer to store read data |
| count | size_t | Maximum number of bytes to read |

## Description

`read()` attempts to read up to `count` bytes from file descriptor `fd` into the buffer starting at `buf`. The behavior depends on the file descriptor type:

- **Regular files**: Reads from the current file offset, which is advanced by the number of bytes read
- **Pipes/sockets**: Returns available data without waiting (non-blocking) or blocks until data arrives
- **Character devices**: Reads from the device FIFO
- **Directories**: Returns directory entries (see `getdents`)

## Return Value

On success, the number of bytes read is returned (0 indicates end of file). On error, -1 is returned and errno is set.

## Errors

| Error | Condition |
|-------|-----------|
| EBADF | fd is not a valid file descriptor or not readable |
| EFAULT | buf points outside accessible address space |
| EINTR | Interrupted by a signal before any data was read |
| EIO | I/O error occurred |
| EISDIR | fd refers to a directory |
