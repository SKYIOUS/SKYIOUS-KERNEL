# Building and Running Userspace Programs

Userspace programs in SkyOS are ELF binaries linked against the SkyOS libc.

## The libc Library

SkyOS provides a minimal C standard library (`userspace/libc/`) that implements:
- Standard I/O (`printf`, `scanf`, file operations)
- Memory management (`malloc`, `free`)
- String manipulation (`strcpy`, `strlen`, `memcmp`)
- POSIX syscall wrappers (`read`, `write`, `open`, `mmap`)

## Writing a Userspace Program

```c
#include <skyos.h>

int main(int argc, char** argv) {
    printf("Hello from SkyOS userspace!\n");
    return 0;
}
```

Build with the cross-compilation toolchain:

```bash
x86_64-skyos-gcc -o hello hello.c
```

## Loading and Execution

The kernel's ELF loader reads the binary, maps segments into the process address space, and jumps to the entry point. The init process (`/sbin/init`) is loaded by the kernel at boot.

## Init System

The init system starts essential userspace services:
1. Device manager (`devmand`)
2. Display server
3. Network manager
4. Login shell

## Environment Variables

The kernel passes a minimal environment to the init process including `PATH=/bin:/sbin` and `HOME=/root`.
