# write() Syscall

Write data to a file descriptor.

## Synopsis

```c
ssize_t write(int fd, const void *buf, size_t count);
```

## Arguments

| Argument | Type | Description |
|----------|------|-------------|
| fd | int | File descriptor to write to |
| buf | const void* | Buffer containing data to write |
| count | size_t | Number of bytes to write |

## Description

`write()` writes up to `count` bytes from the buffer starting at `buf` to the file descriptor `fd`. For regular files, the write starts at the current file offset. For sockets, the data is sent through the network connection. For character devices, the data is written to the device output FIFO.

For pipes and FIFOs, if the write would exceed `PIPE_BUF` bytes and `O_NONBLOCK` is set, the write may be partial. If `O_NONBLOCK` is not set, the write blocks until space is available.

## Return Value

On success, the number of bytes written is returned (may be less than `count`). On error, -1 is returned and errno is set.

## Errors

| Error | Condition |
|-------|-----------|
| EBADF | fd is not a valid file descriptor or not writable |
| EFAULT | buf points outside accessible address space |
| EINTR | Interrupted by a signal before completion |
| EIO | I/O error occurred |
| ENOSPC | No space left on device |
| EPIPE | Broken pipe (peer closed connection) |
