# Kernel Architecture Overview

SkyOS follows a hybrid microkernel design philosophy. The kernel is built as a single privileged executable that provides core services (memory management, scheduling, IPC) while delegating traditional OS services (filesystems, drivers, networking) to userspace tasks.

## Design Philosophy

The architecture prioritizes three core principles:

- **Safety**: By leveraging Rust's type system and ownership model, we eliminate entire classes of bugs at compile time. The kernel is written almost entirely in safe Rust, with `unsafe` blocks confined to hardware interaction and low-level initialization.
- **Asynchrony**: The entire kernel is built around an async/await executor. System calls, interrupt handlers, and driver operations are non-blocking, enabling high concurrency without threading overhead.
- **Modularity**: Each subsystem is isolated behind well-defined interfaces. Drivers, filesystems, and protocol implementations can be loaded and managed independently.

## Kernel Structure

The kernel is organized into these major layers:

1. **Arch Layer** (`src/arch/x86_64/`): Platform-specific code including GDT, IDT, paging, and CPU initialization.
2. **Core Layer** (`src/kernel/`): Platform-independent kernel primitives including memory management, scheduling, and synchronization.
3. **Services Layer** (`src/syscalls/`, `src/ipc/`): System call dispatch and inter-process communication.
4. **Driver Framework** (`src/drivers/`): Hardware abstraction and device driver infrastructure.
5. **Filesystem Layer** (`src/vfs/`): Virtual file system interface and implementations.

## Microkernel vs Monolithic

SkyOS takes a pragmatic approach: core kernel subsystems (scheduling, memory, IPC) run in kernel space for performance, while filesystem implementations, network stacks, and drivers run as userspace tasks. This hybrid approach provides the isolation benefits of a microkernel without sacrificing performance for fundamental operations.
