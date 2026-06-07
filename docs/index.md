# SkyOS Documentation

Welcome to the SkyOS kernel documentation. SkyOS is a modern, microkernel-based operating system written in Rust, designed with a focus on safety, performance, and asynchronous operation.

## Table of Contents

### [Architecture](architecture/overview.md)
Design overview of the SkyOS kernel, including memory management, process/thread model, scheduling, interrupt handling, syscall interface, SMP support, IPC, synchronization, and timekeeping.

- [Kernel Architecture Overview](architecture/overview.md)
- [Memory Management](architecture/memory.md)
- [Process & Thread Model](architecture/process.md)
- [Scheduler Design](architecture/scheduling.md)
- [Interrupt Handling](architecture/interrupts.md)
- [System Call Interface](architecture/syscall.md)
- [SMP Support](architecture/smp.md)
- [Inter-process Communication](architecture/ipc.md)
- [Synchronization Primitives](architecture/sync.md)
- [Timekeeping & Timers](architecture/time.md)

### [Guides](guide/getting_started.md)
Practical guides for building, running, and extending the kernel.

- [Getting Started](guide/getting_started.md)
- [QEMU Setup](guide/qemu_setup.md)
- [Adding a Syscall](guide/adding_syscall.md)
- [Writing a Driver](guide/writing_driver.md)
- [Debugging](guide/debugging.md)
- [Contributing](guide/contributing.md)
- [Coding Style](guide/coding_style.md)
- [Testing](guide/testing.md)
- [VFS Guide](guide/vfs_guide.md)
- [Userspace](guide/userspace.md)

### [API Reference](api/syscalls_overview.md)
Complete API reference for system calls, VFS, drivers, and the userspace libc.

- [Syscalls Overview](api/syscalls_overview.md)
- [read() Syscall](api/syscall_read.md)
- [write() Syscall](api/syscall_write.md)
- [open() Syscall](api/syscall_open.md)
- [mmap() Syscall](api/syscall_mmap.md)
- [execve() Syscall](api/syscall_exec.md)
- [GUI Syscalls](api/gui_syscalls.md)
- [VFS API](api/vfs_api.md)
- [Driver API](api/driver_api.md)
- [libc API](api/libc_api.md)

### [Design Documents](design/philosophy.md)
In-depth exploration of design decisions and philosophy behind SkyOS.

- [Design Philosophy](design/philosophy.md)
- [Why Rust](design/why_rust.md)
- [Async Model](design/async_model.md)
- [VFS Design](design/vfs_design.md)
- [Memory Safety](design/memory_safety.md)
- [GUI Architecture](design/gui_architecture.md)
- [Networking Stack](design/networking_stack.md)
- [Driver Model](design/driver_model.md)
- [ELF Loading](design/elf_loading.md)
- [Error Handling](design/error_handling.md)

### [Future Plans](future/roadmap.md)
Roadmap and phased development goals.

- [Roadmap](future/roadmap.md)
- [Phase 1: Kernel Stabilization](future/phase1_stable.md)
- [Phase 2: Networking](future/phase2_networking.md)
- [Phase 3: GUI](future/phase3_gui.md)
- [Phase 4: Userspace](future/phase4_userspace.md)
- [Phase 5: Drivers](future/phase5_drivers.md)
- [Phase 6: Security](future/phase6_security.md)
- [Phase 7: Performance](future/phase7_performance.md)
- [Phase 8: Portability](future/phase8_portability.md)
- [Long-term Vision](future/long_term.md)

### [Syscall Reference](syscalls/index.md)
Complete syscall table and individual syscall documentation.

- [Syscall Table](syscalls/index.md)
- [I/O Syscalls](syscalls/io_syscalls.md)
- [Memory Syscalls](syscalls/mem_syscalls.md)
- [Process Syscalls](syscalls/proc_syscalls.md)
- [Filesystem Syscalls](syscalls/file_syscalls.md)
- [Network Syscalls](syscalls/net_syscalls.md)
- [GUI Syscalls](syscalls/gui_syscalls.md)
- [Time Syscalls](syscalls/time_syscalls.md)
- [UID/GID Syscalls](syscalls/uid_gid.md)
- [Misc Syscalls](syscalls/misc_syscalls.md)

### [Drivers](drivers/overview.md)
Documentation for all kernel drivers.

- [Driver Overview](drivers/overview.md)
- [PS/2 Controller](drivers/ps2.md)
- [Mouse](drivers/mouse.md)
- [Keyboard](drivers/keyboard.md)
- [Graphics (Framebuffer)](drivers/graphics.md)
- [RTC](drivers/rtc.md)
- [Intel e1000](drivers/e1000.md)
- [VirtIO Net](drivers/virtio_net.md)
- [PCI](drivers/pci.md)
- [ACPI](drivers/acpi.md)

### [Build System](build/overview.md)
How to build, configure, and troubleshoot the kernel.

- [Build Overview](build/overview.md)
- [Prerequisites](build/prerequisites.md)
- [Building](build/building.md)
- [Boot Images](build/bootimage.md)
- [Configuration](build/configuration.md)
- [Cross-compilation](build/cross_compilation.md)
- [Userspace Build](build/userspace_build.md)
- [Docker Setup](build/docker_setup.md)
- [Troubleshooting](build/troubleshooting.md)
- [Optimizations](build/optimizations.md)

### [Testing](testing/overview.md)
Quality assurance and testing methodology.

- [Testing Overview](testing/overview.md)
- [Unit Tests](testing/unit_tests.md)
- [Integration Tests](testing/integration.md)
- [Memory Testing](testing/memory_testing.md)
- [Syscall Testing](testing/syscall_testing.md)
- [Network Testing](testing/network_testing.md)
- [Stress Testing](testing/stress_testing.md)
- [Regression Testing](testing/regression.md)
- [Code Coverage](testing/coverage.md)
- [CI/CD](testing/ci_cd.md)

### [Reference](reference/x86_64.md)
Technical reference materials.

- [x86_64 Reference](reference/x86_64.md)
- [UEFI Reference](reference/uefi.md)
- [ELF Reference](reference/elf.md)
- [PCI IDs](reference/pci_ids.md)
- [PS/2 Scan Codes](reference/ps2_codes.md)
- [I/O Port Map](reference/io_ports.md)
- [IRQ Table](reference/irq_table.md)
- [Memory Map](reference/memory_map.md)
- [File Formats](reference/file_formats.md)
- [Glossary](reference/glossary.md)

### [Contributing](contributing/code_of_conduct.md)
How to contribute to SkyOS.

- [Code of Conduct](contributing/code_of_conduct.md)
- [Pull Requests](contributing/pull_requests.md)
- [Issue Tracking](contributing/issue_tracking.md)
- [Maintainers](contributing/maintainers.md)
- [License](contributing/license.md)

### [Security](security/overview.md)
Security architecture and mechanisms.

- [Security Overview](security/overview.md)
- [Memory Protection](security/memory_protection.md)
- [Syscall Security](security/syscall_security.md)
- [User Isolation](security/user_isolation.md)
- [Future Security](security/future_security.md)
