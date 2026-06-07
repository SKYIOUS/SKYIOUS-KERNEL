# I/O System Calls

The I/O syscalls provide file descriptor operations.

## read (syscall 0)

```c
ssize_t read(int fd, void *buf, size_t count);
```

Reads up to `count` bytes from file descriptor `fd` into `buf`. Returns bytes read, 0 at EOF, or -1 on error.

## write (syscall 1)

```c
ssize_t write(int fd, const void *buf, size_t count);
```

Writes up to `count` bytes from `buf` to file descriptor `fd`. Returns bytes written (may be less than count).

## open (syscall 2)

```c
int open(const char *pathname, int flags, mode_t mode);
```

Opens the file at `pathname` with the specified flags. Returns a file descriptor or -1 on error.

## close (syscall 3)

```c
int close(int fd);
```

Closes the file descriptor `fd`. Returns 0 on success, -1 on error.

## lseek (syscall 8)

```c
off_t lseek(int fd, off_t offset, int whence);
```

Repositions the file offset for `fd`. `whence` can be SEEK_SET, SEEK_CUR, or SEEK_END.

## poll (syscall 7)

```c
int poll(struct pollfd *fds, nfds_t nfds, int timeout);
```

Waits for I/O events on multiple file descriptors. Returns the number of ready descriptors.

## ioctl (syscall 16)

```c
int ioctl(int fd, unsigned long request, ...);
```

Performs device-specific control operations on file descriptor `fd`.

## pipe (syscall 22)

```c
int pipe(int pipefd[2]);
```

Creates a unidirectional data channel. `pipefd[0]` is the read end, `pipefd[1]` is the write end.

## dup/dup2 (syscalls 32-33)

```c
int dup(int oldfd);
int dup2(int oldfd, int newfd);
```

Duplicate a file descriptor. `dup2` allows specifying the target descriptor number.
