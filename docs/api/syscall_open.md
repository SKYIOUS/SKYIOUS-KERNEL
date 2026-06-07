# open() Syscall

Open or create a file.

## Synopsis

```c
int open(const char *pathname, int flags, ... mode_t mode);
```

## Arguments

| Argument | Type | Description |
|----------|------|-------------|
| pathname | const char* | Path to the file to open |
| flags | int | Access mode and flags |
| mode | mode_t | Permission bits (optional, for creation) |

## Flags

| Flag | Value | Description |
|------|-------|-------------|
| O_RDONLY | 0 | Open for reading only |
| O_WRONLY | 1 | Open for writing only |
| O_RDWR | 2 | Open for reading and writing |
| O_CREAT | 0100 | Create file if it doesn't exist |
| O_EXCL | 0200 | Fail if O_CREAT and file exists |
| O_TRUNC | 01000 | Truncate file to zero length |
| O_APPEND | 02000 | Append writes to end of file |
| O_NONBLOCK | 04000 | Non-blocking mode |
| O_DIRECTORY | 0200000 | Must be a directory |

## Description

`open()` opens the file specified by `pathname`. If the file does not exist and `O_CREAT` is specified, the file is created with the mode specified by the `mode` argument. The file descriptor returned is the lowest-numbered unused descriptor.

## Return Value

Returns a non-negative file descriptor on success. On error, -1 is returned and errno is set.

## Errors

| Error | Condition |
|-------|-----------|
| EACCES | Permission denied |
| EEXIST | O_CREAT and O_EXCL were specified but file exists |
| ENOENT | File not found and O_CREAT not specified |
| ENOMEM | Insufficient kernel memory |
| ENOSPC | No space for new file creation |
