# mmap() Syscall

Map files or devices into memory.

## Synopsis

```c
void *mmap(void *addr, size_t length, int prot, int flags, int fd, off_t offset);
```

## Arguments

| Argument | Type | Description |
|----------|------|-------------|
| addr | void* | Hint for the starting address |
| length | size_t | Length of the mapping in bytes |
| prot | int | Memory protection flags |
| flags | int | Mapping type and options |
| fd | int | File descriptor to map |
| offset | off_t | Offset into the file |

## Description

`mmap()` creates a new mapping in the virtual address space of the calling process. The mapping can be:

- **File-backed**: Maps the contents of a file (specified by `fd`) into memory
- **Anonymous**: Not backed by any file (`MAP_ANONYMOUS`), initially zero-filled

## Protection Flags

| Flag | Description |
|------|-------------|
| PROT_READ | Pages may be read |
| PROT_WRITE | Pages may be written |
| PROT_EXEC | Pages may be executed |
| PROT_NONE | Pages may not be accessed |

## Return Value

Returns the starting address of the mapping on success. On error, returns `MAP_FAILED` (i.e., `(void*)-1`).

## Errors

| Error | Condition |
|-------|-----------|
| EACCES | File not open for reading (PROT_READ) or writing (PROT_WRITE) |
| EINVAL | Invalid arguments (bad flags, length zero) |
| ENOMEM | Insufficient memory |
| ENODEV | Backing filesystem doesn't support mapping |
