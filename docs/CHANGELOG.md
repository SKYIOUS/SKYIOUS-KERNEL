# SkyOS Changelog

## [0.1.0] - In Development

### Phase A: Foundation Hardening
-   **A1:** Upgraded `bootloader` to v0.11 for UEFI support.
-   **A2:** Implemented a Buddy Allocator for physical frame management.
-   **A3:** Implemented a Slab Allocator for efficient kernel object allocation.
-   **A4:** Defined and documented the kernel virtual address space layout.
-   **A5:** Integrated LAPIC and IOAPIC for modern interrupt handling, disabling the legacy PIC.

### Phase B: Process & Memory Model
-   **B1:** Implemented per-process user page tables with a shared higher-half kernel mapping.
-   **B2:** Added a Page Fault handler with demand paging support.
-   **B3:** Implemented Copy-on-Write (CoW) for efficient `fork`.
-   **B4:** Established a `Process` model with a global process table.
-   **B5:** Created an ELF loader to execute user-space programs.
-   **B6:** Implemented `sys_fork`, `sys_execve`, `sys_exit`, and `sys_wait4`.

### Phase C: Scheduler Evolution
-   **C1:** Implemented a priority-based, preemptive round-robin scheduler.
-   **C2:** Added `sys_nanosleep` for thread blocking.
-   **C4:** Implemented SMP support, allowing all cores to participate in scheduling.

### Phase D: Syscall Expansion
-   Implemented a wide range of POSIX-compatible syscalls for processes, files, memory, and IPC, including `pipe` and `dup2`.

### Phase E: Filesystem Layer
-   Implemented a trait-based VFS with support for `Tmpfs`, `Ext2` (read-only), `FAT32`, and `Pipe`.

### Phase F: Networking Stack
-   Integrated the `smoltcp` networking stack with E1000 and VirtIO drivers. Implemented `socket` syscalls.

### Phase G: Graphical System
-   Implemented a framebuffer driver using UEFI GOP and a graphical console.

### Phase H: Korlang Integration
-   Added a dedicated `sys_korlang` syscall for runtime support.

### Phase I: Security
-   Enabled SMEP/SMAP.
-   Implemented Kernel Stack Guard Pages.
-   Added basic syscall input validation.

### Documentation
-   Created comprehensive design documents for architecture, syscalls, VFS, scheduling, and the build process.
