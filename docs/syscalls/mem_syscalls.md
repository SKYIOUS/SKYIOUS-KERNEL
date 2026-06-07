# Memory System Calls

The memory syscalls manage virtual memory mappings.

## mmap (syscall 9)

```c
void *mmap(void *addr, size_t length, int prot, int flags, int fd, off_t offset);
```

Creates a new mapping in the virtual address space. Supports:
- Anonymous mappings (`MAP_ANONYMOUS`)
- File-backed mappings
- Shared (`MAP_SHARED`) and private (`MAP_PRIVATE`) mappings
- Fixed address mappings (`MAP_FIXED`)

## munmap (syscall 11)

```c
int munmap(void *addr, size_t length);
```

Unmaps previously mapped memory. The address must be page-aligned.

## mprotect (syscall 10)

```c
int mprotect(void *addr, size_t len, int prot);
```

Changes access protections for a memory region. Protection flags: `PROT_NONE`, `PROT_READ`, `PROT_WRITE`, `PROT_EXEC`.

## brk (syscall 12)

```c
int brk(void *addr);
void *sbrk(intptr_t increment);
```

Changes the program break (end of the data segment). Used by `malloc()` for heap management.

## mremap (syscall 25)

```c
void *mremap(void *old_addr, size_t old_size, size_t new_size, int flags, ...);
```

Expands or shrinks an existing memory mapping, potentially moving it to a new address.

## msync (syscall 26)

```c
int msync(void *addr, size_t length, int flags);
```

Synchronizes a mapped file with the backing storage. Flags: `MS_ASYNC`, `MS_SYNC`, `MS_INVALIDATE`.

## madvise (syscall 28)

```c
int madvise(void *addr, size_t length, int advice);
```

Gives advice about expected memory usage patterns. Advice values: `MADV_NORMAL`, `MADV_RANDOM`, `MADV_SEQUENTIAL`, `MADV_WILLNEED`, `MADV_DONTNEED`.

## Shared Memory

```c
int shmget(key_t key, size_t size, int shmflg);
void *shmat(int shmid, const void *shmaddr, int shmflg);
int shmdt(const void *shmaddr);
```

System V shared memory interface. `shmget` allocates, `shmat` attaches, `shmdt` detaches.
