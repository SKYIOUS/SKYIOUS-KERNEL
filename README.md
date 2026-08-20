# Vahi Kernel

> A modern, monolithic Rust kernel â€” the core of **SARGA OS**.
> Multi-architecture, feature-rich, and built for performance and safety.

<div align="center">

[![Rust](https://img.shields.io/badge/Rust-nightly-dea584?logo=rust&logoColor=fff)](https://www.rust-lang.org)
[![Arch](https://img.shields.io/badge/arch-x86__64%20%7C%20aarch64-blueviolet)](#)
[![License: SSL](https://img.shields.io/badge/license-SSL-green)](#)
[![Syscalls](https://img.shields.io/badge/syscalls-90%2B-blue)](#)
[![Drivers](https://img.shields.io/badge/drivers-12%2B-orange)](#)
[![Filesystems](https://img.shields.io/badge/fs-7-yellowgreen)](#)
[![Build](https://img.shields.io/badge/build-passing-brightgreen)](#)

</div>

---

## Table of Contents

- [Overview](#overview)
- [How This Project Was Built](#how-this-project-was-built)
- [Architecture](#architecture)
- [Features](#features)
  - [Syscalls](#syscalls)
  - [Filesystems](#filesystems)
  - [Drivers](#drivers)
  - [Process & Scheduler](#process--scheduler)
  - [Memory Management](#memory-management)
  - [Security](#security)
  - [Networking](#networking)
  - [GUI Compositor](#gui-compositor)
  - [eBPF](#ebpf)
  - [Linux Compatibility](#linux-compatibility)
  - [ASH Sandbox](#ash-sandbox)
  - [io_uring](#io_uring)
- [Build & Run](#build--run)
- [Project Structure](#project-structure)
- [Documentation](#documentation)
- [Testing](#testing)
- [Architecture Portability](#architecture-portability)
- [Contributing](#contributing)
- [License](#license)

---

## Overview

**Vahi** (Sanskrit: "the carrier") is a monolithic kernel written entirely in Rust. It powers **SARGA OS** â€” a modern operating system built from scratch with a focus on safety, performance, and extensibility.

### Design Philosophy

- **Safety first**: Memory safety through Rust's ownership model, not garbage collection
- **Monolithic but modular**: All core services in kernel space, clean internal abstractions
- **POSIX-inspired**: Linux-compatible syscall numbering and ABI where practical
- **Multi-architecture**: x86_64 primary, aarch64 in progress, RISC-V planned
- **Self-hosting**: Full userspace environment built alongside the kernel

### Key Numbers

| Metric | Value |
|--------|-------|
| Lines of Rust | ~50,000+ |
| Syscalls | 90+ |
| Filesystems | 7 (SkyFS, ext2, FAT32, tarfs, ramfs, devfs, ctlfs) |
| Drivers | 12+ (storage, net, audio, USB, GPU, input) |
| Kernel threads | Async executor + scheduler |
| Supported archs | x86_64 (mature), aarch64 (in progress) |
| Boot protocol | UEFI (via `bootloader` crate) |

## How This Project Was Built
>
> The vast majority of this codebase was generated with the assistance of AI (large language models).
> This allowed a single developer to create a full monolithic kernel from scratch â€” something that
> would normally require a team of engineers over many years.
>
> **We are looking for human contributors.** If you understand Rust, operating systems, or any part
> of this codebase â€” whether you wrote none of it or all of it â€” your help is needed and welcome.
> We are actively seeking people to:
>
> - Review the code for correctness, security, and performance
> - Fix bugs, edge cases, and incomplete implementations
> - Refactor AI-generated code into idiomatic, maintainable Rust
> - Add tests, documentation, and missing features
> - Port the kernel to real hardware, not just QEMU
> - Help transition this from an AI-driven prototype to a community-maintained project
>
> No contribution is too small. Open an issue, submit a PR, or start a discussion.
> See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

---

## Architecture

```
â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”
â”‚                     Vahi Kernel                              â”‚
â”‚                                                              â”‚
â”‚  â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”   â”‚
â”‚  â”‚                  Syscall Layer                        â”‚   â”‚
â”‚  â”‚  90+ syscalls: read/write/open/mmap/fork/execve/net/  â”‚   â”‚
â”‚  â”‚  gui/clone/futex/io_uring/bpf                       â”‚   â”‚
â”‚  â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜   â”‚
â”‚                                                              â”‚
â”‚  â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â” â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â” â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â” â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”   â”‚
â”‚  â”‚  Scheduler â”‚ â”‚  Memory  â”‚ â”‚  VFS   â”‚ â”‚  Network       â”‚   â”‚
â”‚  â”‚  Preemptiveâ”‚ â”‚  Buddy   â”‚ â”‚ 7 FS   â”‚ â”‚  smoltcp       â”‚   â”‚
â”‚  â”‚  8 prio    â”‚ â”‚  Slab    â”‚ â”‚ mounts â”‚ â”‚  E1000/VirtIO  â”‚   â”‚
â”‚  â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜ â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜ â””â”€â”€â”€â”€â”€â”€â”€â”€â”˜ â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜   â”‚
â”‚                                                              â”‚
â”‚  â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â” â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â” â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â” â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”   â”‚
â”‚  â”‚  Drivers   â”‚ â”‚  GUI     â”‚ â”‚ eBPF   â”‚ â”‚  Security      â”‚   â”‚
â”‚  â”‚  12+ devs  â”‚ â”‚Compositorâ”‚ â”‚ VM+Ver â”‚ â”‚  SMEP/UMIP/    â”‚   â”‚
â”‚  â”‚  PCI/ACPI  â”‚ â”‚ 30 FPS   â”‚ â”‚ Map+Hlpâ”‚ â”‚  ASLR/Caps     â”‚   â”‚
â”‚  â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜ â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜ â””â”€â”€â”€â”€â”€â”€â”€â”€â”˜ â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜   â”‚
â”‚                                                              â”‚
â”‚  â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”  â”‚
â”‚  â”‚              Arch Abstraction (Arch trait)             â”‚  â”‚
â”‚  â”‚  x86_64 (SYSCALL/SYSRET, FSGSBASE)                     â”‚  â”‚
â”‚  â”‚  aarch64 (SVC/ERET, TPIDR_EL0, GICv2/v3)               â”‚  â”‚
â”‚  â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜  â”‚
â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜
```

### Boot Flow

```
UEFI firmware
    â”‚
    â–¼
bootloader crate (UEFI boot protocol)
    â”‚
    â–¼
kernel_main()
    â”œâ”€â”€ KASLR init (RDTSC entropy)
    â”œâ”€â”€ CPUID feature detection (SMEP, UMIP, FSGSBASE)
    â”œâ”€â”€ Memory init (OffsetPageTable, physical map)
    â”œâ”€â”€ Framebuffer init (UEFI GOP)
    â”œâ”€â”€ Frame allocator init (Buddy)
    â”œâ”€â”€ Heap init (linked_list_allocator @ 0xFFFF_C000_0000_0000)
    â”œâ”€â”€ GDT + TSS init
    â”œâ”€â”€ IDT + PIC init (exception handlers, IRQs)
    â”œâ”€â”€ Syscall init (STAR/LStar/SFMask MSRs, SYSCALL entry)
    â”œâ”€â”€ ACPI init (RSDP parse, FADT, MADT)
    â”œâ”€â”€ APIC init (LAPIC + I/O APIC)
    â”œâ”€â”€ SMP boot (SIPI to APs)
    â”œâ”€â”€ PS/2 init (keyboard + mouse)
    â”œâ”€â”€ PCI enumeration (scan bus, init drivers)
    â”œâ”€â”€ VFS init (mount initrd, devfs, ctlfs, tmpfs, partitions)
    â”œâ”€â”€ Network init (smoltcp, E1000)
    â”œâ”€â”€ LSM init
    â”œâ”€â”€ GUI init (compositor, window manager, desktop)
    â”œâ”€â”€ Spawn async tasks (kernel shell, GUI refresh, network poll)
    â”œâ”€â”€ Spawn init_os_task (loads /bin/init into userspace)
    â”œâ”€â”€ Enable interrupts (sti)
    â””â”€â”€ Enter scheduler (never returns)
```

### Memory Layout

```
0x0000_0000_0000 â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”
                 â”‚   Userspace          â”‚
                 â”‚   (per-process)      â”‚
0x7FFF_FFFF_E000 â”œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”¤
                 â”‚   Kernel Mapping     â”‚
0xFFFF_8000_0000 â”œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”¤
                 â”‚   Physical Memory    â”‚
                 â”‚   (1:1 mapped)       â”‚
0xFFFF_C000_0000 â”œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”¤
                 â”‚   Kernel Heap        â”‚
                 â”‚   (Buddy + Slab)     â”‚
0xFFFF_FFFF_FFFF â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜
```

---

## Features

### Syscalls

The kernel provides 90+ syscalls with Linux-compatible numbering. The syscall ABI is **frozen for v1.0**.

| Category | Syscalls |
|----------|----------|
| **File I/O** | `read` `write` `open` `close` `stat` `fstat` `lseek` `ioctl` `access` `pipe` `select` `poll` `dup` `dup2` `fcntl` `getcwd` `chdir` `rename` `mkdir` `unlink` `symlink` `readlink` `fchmod` `fchown` `statfs` `mount` `umount2` `getdents64` |
| **Memory** | `mmap` `munmap` `brk` |
| **Process** | `getpid` `getppid` `clone` `fork` `execve` `exit` `exit_group` `wait4` `set_tid_address` `getuid` `getgid` `setuid` `setgid` `geteuid` `getegid` `sched_setattr` `sched_getattr` `arch_prctl` |
| **Signals** | `rt_sigaction` `rt_sigreturn` `kill` |
| **Networking** | `socket` `connect` `accept` `sendto` `recvfrom` `bind` `listen` `resolve` |
| **Timing** | `sched_yield` `nanosleep` `clock_gettime` |
| **Sync** | `futex` (WAIT/WAKE/CMP_REQUEUE) `sync` |
| **GUI** | `gui_create_window` `gui_get_buffer` `gui_flush` `gui_map_buffer` `gui_get_key` `gui_get_mouse` `gui_set_title` `gui_destroy_window` `gui_resize_window` `gui_move_window` `clipboard` `notify` |
| **Audio** | `beep` |
| **GPU** | `drmctl` |
| **Crypto** | `hash` (SHA-256, PBKDF2) |
| **PTY** | `openpty` |
| **Filesystem** | `mkfs` |
| **Kernel** | `uname` `sysinfo` `reboot` |
| **eBPF** | `bpf` |
| **io_uring** | `io_uring_setup` `io_uring_enter` |

### Filesystems

| Filesystem | Type | Features |
|------------|------|----------|
| **SkyFS** | Journaling read-write | Custom filesystem with B-tree extent storage, WAL journaling, block allocator, inline data, format utility |
| **Ext2** | Read-only | Linux ext2 for root/partition mounts |
| **FAT32** | Read-write | Via `fatfs` crate, MBR/GPT partition support |
| **TarFS** | Read-only in-memory | Embedded initrd, also mounts tar from block devices |
| **RamFS/Tmpfs** | In-memory read-write | `/tmp`, `/run` mounts |
| **DevFS** | Virtual | `/dev` with device nodes |
| **CtlFS** | Virtual | Plan9-style `/ctl` control filesystem (replaces /proc + /sys) |

### Drivers

#### Storage

| Driver | Description |
|--------|-------------|
| **AHCI** | SATA controller driver with NCQ, PRD-based DMA |
| **NVMe** | NVMe SSD driver with admin/I/O queues, PRP DMA, BlockDevice trait |
| **VirtIO-Block** | Para-virtualized block device for QEMU/KVM |

#### Networking

| Driver | Description |
|--------|-------------|
| **Intel E1000** | 82540EM Gigabit Ethernet with TX/RX descriptor rings, interrupts |
| **VirtIO-Net** | Para-virtualized network device |

#### Input

| Driver | Description |
|--------|-------------|
| **PS/2 Keyboard** | Scancode translation, modifier keys, ring buffer |
| **PS/2 Mouse** | Relative movement, button events, wheel support |

#### Audio

| Driver | Description |
|--------|-------------|
| **HDA** | Intel High Definition Audio â€” playback, volume control (0-100%), stream halt |
| **PC Speaker** | Legacy programmable interval timer beeper |

#### Display

| Driver | Description |
|--------|-------------|
| **UEFI GOP** | Framebuffer from boot services, linear 32bpp |
| **VirtIO GPU** | Para-virtualized GPU with 2D commands, cursor, scanout |

#### USB

| Driver | Description |
|--------|-------------|
| **xHCI** | USB 3.0 controller â€” device descriptor parsing, config walking, HID/mass storage class detection |

#### Other

| Driver | Description |
|--------|-------------|
| **PCI** | Bus enumeration, configuration space access, BAR detection, MSI/MSI-X |
| **ACPI** | FADT parsing, S5 shutdown, RESET_REG reboot |
| **RTC** | CMOS real-time clock, date/time read |
| **Watchdog** | Timer-based watchdog |

### Process & Scheduler

```
- Preemptive priority-based round-robin scheduler
- 8 priority levels (0 = highest, 7 = idle)
- Per-CPU run queues + global queue
- Time quantum configurable per priority
- Cooperative async executor with YieldNow primitive
- Copy-on-Write fork
- Demand paging (page faults map on access)
- Thread-local storage via FS/GS base
- Per-process UID/GID/EUID/EGID
- File descriptor tables per process
- Virtual memory area (VMA) tracking per process
- CLONE_VM for thread creation
- clear_child_tid + futex for pthread_join
- Linux emulation mode per-process
```

### Memory Management

```
- Physical: Buddy frame allocator (order 0-10)
- Kernel heap: Slab allocator + linked_list_allocator
- Virtual: OffsetPageTable (4-level paging)
- KASLR: Randomized kernel base via RDTSC entropy
- Stack canary: __stack_chk_guard seeded at boot
- Guard pages on kernel stacks
- SMAP/SMEP/UMIP hardware protections
- Virt-to-phys via OffsetPageTable::translate_addr
- DMA buffers: DmaBuf and RingBuf RAII containers
```

### Security

| Feature | Description |
|---------|-------------|
| **SMEP** | Supervisor Mode Execution Prevention (CR4 bit 20) |
| **SMAP** | Supervisor Mode Access Prevention (EFLAGS.AC) |
| **UMIP** | User-Mode Instruction Prevention (CR4 bit 11) |
| **FSGSBASE** | FS/GS base instructions (when available, MSR fallback) |
| **KASLR** | Kernel ASLR via RDTSC + sequential entropy mixing |
| **Stack Canary** | `__stack_chk_guard` with randomized seed |
| **Capabilities** | CAP_SYS_ADMIN, CAP_KILL, CAP_SYS_BOOT, CAP_SETUID, CAP_SETGID with audit logging |
| **LSM** | Linux Security Module skeleton with policy loading from `/etc/lsm_policy` |
| **Audit** | Security events logged to serial with PID context |
| **User Memory Safety** | SMAP-safe read/write via `user_access` module with bounds checking |

### Networking

```
- Integrated smoltcp TCP/IP stack
- IPv4, ICMP, UDP, TCP
- DHCPv4 client
- DNS resolver (getaddrinfo)
- Socket syscalls: socket, bind, connect, listen, accept, sendto, recvfrom
- Loopback interface
- Static IP configuration
- ARP cache
```

### GUI Compositor

```
- Full compositing window manager at 30 FPS
- Per-window framebuffers with damage tracking
- Mouse cursor with hardware cursor support
- Keyboard input routing
- Window management: create, destroy, resize, move, minimize, close
- Title bars with hover effects
- Desktop wallpaper and icons
- Terminal emulator integrated
- Splash screen at boot
- Notification system (toast popups)
- Clipboard support
```

The GUI is rendered entirely in kernel space â€” no userspace display server needed. Each window gets a dedicated framebuffer, and the compositor blends them together at 30 FPS.

### eBPF

```
- In-kernel eBPF virtual machine
- Verifier with safety checks:
  - Bounds checking on all memory access
  - R10 frame pointer write protection
  - CALL helper ID validation
  - Loop detection (backwards jumps forbidden)
- Four built-in helpers:
  - map_lookup: Look up entries in eBPF maps
  - getpid: Get current process ID
  - get_ticks: Get system timer ticks
  - debug_print: Print to serial console
- eBPF maps (array/hash)
- sys_bpf syscall (#321)
```

### Linux Compatibility

The kernel includes a per-process Linux emulation mode, auto-detected via ELF interpreter at `execve`:

```
- EmulationMode enum: Native | Linux | Windows
- Linux ELF detection via PT_INTERP interpreter string
- 65-entry Linux-to-Vahi syscall mapping table
- Direct handlers for:
  - sys_uname (returns "Linux" / "5.15.0-sarga")
  - sys_arch_prctl (ARCH_SET_FS / ARCH_GET_FS for glibc TLS)
  - sys_fork (via clone(SIGCHLD))
  - sys_rt_sigaction (translates Linux sigaction struct with SA_RESTORER)
  - sys_rt_sigreturn (restores SignalContext)
- Remaining 60+ syscalls routed through do_syscall
```

### ASH Sandbox

```
- Native helper sandbox engine (gated by ash feature)
- SYS_ASH_* syscalls for rule enforcement
```

### io_uring

```
- Linux-compatible io_uring setup/enter syscalls
- SYS_IO_URING_SETUP (#425)
- SYS_IO_URING_ENTER (#426)
```

---

## Build & Run

### Prerequisites

- Rust nightly (rustup default nightly)
- `rust-src` component
- `llvm-tools-preview` component
- `x86_64-unknown-none` target
- QEMU (for testing)

### Setup

```bash
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
rustup component add llvm-tools-preview --toolchain nightly
rustup target add x86_64-unknown-none --toolchain nightly
```

### Build the Kernel

```bash
cd kernel
cargo build                     # Debug build
cargo build --release           # Release build with LTO
```

### Build Bootimage

```bash
# Using the builder crate
cd builder
cargo run -- ../kernel/target/x86_64-unknown-none/debug/vahi_kernel
```

Or use the convenience scripts:

```powershell
# Windows
.\make_bootimage.ps1

# Linux/WSL
./make_bootimage.sh
```

### Full Build (Userspace + Kernel + Bootimage)

```powershell
# 1. Build userspace
.\build_userspace.ps1

# 2. Build kernel
cd kernel
cargo build

# 3. Create bootimage
cd ../builder
cargo run -- ../kernel/target/x86_64-unknown-none/debug/vahi_kernel
```

### Run in QEMU

```powershell
# Quick boot (uses existing bootimage-vahi_kernel.bin)
.\run.ps1

# With display
.\run_qemu_display.ps1

# No display (serial only, for testing)
.\run_test_nographic.ps1
```

#### Direct QEMU commands

```powershell
# Boot from UEFI disk image with serial console
qemu-system-x86_64 -bios OVMF.fd -drive format=raw,file=bootimage-vahi_kernel.bin -m 512M -smp 2 -serial stdio

# Boot with display + serial
qemu-system-x86_64 -bios OVMF.fd -drive format=raw,file=bootimage-vahi_kernel.bin -m 512M -smp 2 -serial file:serial.log

# Full build + boot
.\make_bootimage.ps1; if ($?) { .\run.ps1 }
```

### QEMU Configuration

```
- UEFI boot via OVMF.fd
- 512 MB RAM
- 2 CPU cores (SMP)
- AHCI disk controller
- Intel E1000 NIC (user-mode networking)
- VGA display (GOP framebuffer)
- Serial console for logging
```

## Filesystem Support

### Supported Filesystems

| FS | R/W | Mount point | Notes |
|----|-----|-------------|-------|
| Ext2 | âœ… | Auto-detected on block devices | Full R/W, indirect blocks, mkdir, create |
| Ext4 | Read | Auto-detected | Read-only, extent trees (cfg feature) |
| FAT32 | âœ… | Auto-detected on block devices | Via fatfs crate |
| TarFS | Read | Initrd / block devices | ustar format |
| Tmpfs | âœ… | `/tmp` | In-memory, writable |
| DevFS | âœ… | `/dev` | Device nodes (tty0, fb0, null, zero, block devs) |
| CtlFS | Read | `/ctl` | Plan9-style control files |

### Auto-Mount Behavior

During boot, `vfs::init()` scans all block devices and partitions, auto-detecting and mounting ext2, ext4, FAT32, TarFS, and SkyFS under `/mnt/`. The root filesystem is selected from:

1. Explicit `BOOT_DEVICE` index â†’ tries ext4 â†’ ext2 â†’ SkyFS
2. First block device partition â†’ tries ext4 â†’ ext2 â†’ SkyFS
3. First whole block device â†’ tries ext4 â†’ ext2 â†’ SkyFS
4. Bootloader initrd â†’ TarFS (fallback)

### Testing with an Ext2 Disk Image

```bash
# Create a test ext2 image with known files
python scripts/make_test_ext2.py test_ext2.img 32

# Boot QEMU with the test disk attached
qemu-system-x86_64 -bios OVMF.fd \
    -drive format=raw,file=bootimage-vahi_kernel.bin,if=ide,index=0 \
    -drive format=raw,file=test_ext2.img,if=ide,index=1 \
    -m 512M -smp 2 -serial stdio
```

The kernel auto-mounts the ext2 partition at `/mnt/ext2_0` (or similar). Use the in-kernel shell (`ls`, `cat`, `mkdir`, `touch`) to interact with it.

### Package Manager (.sky packages)

The package format is a ustar tar archive containing a `manifest` file plus payload files. Build and install:

```bash
# Create a package directory
mkdir -p mypkg
echo "name=hello-world" > mypkg/manifest
echo "version=1.0.0" >> mypkg/manifest
echo "description=Example" >> mypkg/manifest
echo 'echo Hello!' > mypkg/hello.sh

# Build the .skp package
python scripts/make_sky_pkg.py mypkg hello-world.skp

# Install (from userspace shell)
spkg install hello-world.skp
```

### In-Kernel Self-Tests

With the `self_test` feature enabled, five ext2 filesystem tests run at boot:

```
ext2_format_mount   â€” Format and mount minimal ext2
ext2_read_file      â€” Read pre-written file, verify content
ext2_write_file     â€” Create, write, read back file
ext2_mkdir_and_stat â€” Create directory, verify stat
ext2_permissions    â€” Verify permission bits in stat
```

See `docs/filesystem-design.md` for the full FS architecture.

---

## Project Structure

```
SKYIOUS KERNEL/
â”œâ”€â”€ kernel/                        # Vahi kernel crate
â”‚   â”œâ”€â”€ Cargo.toml                 # v0.3.0, nightly Rust
â”‚   â”œâ”€â”€ rust-toolchain.toml        # nightly, rust-src, llvm-tools
â”‚   â”œâ”€â”€ build.rs                   # Initrd embedding, hash verification
â”‚   â”œâ”€â”€ linker.ld                  # x86_64 linker script (higher-half)
â”‚   â”œâ”€â”€ aarch64-linker.ld          # aarch64 linker script (physical)
â”‚   â”œâ”€â”€ aarch64-unknown-none.json  # aarch64 target spec
â”‚   â””â”€â”€ src/
â”‚       â”œâ”€â”€ main.rs                # Entry point, boot flow, panic handler
â”‚       â”œâ”€â”€ vga_buffer.rs          # VGA text-mode driver
â”‚       â”œâ”€â”€ interrupts.rs          # IDT, PIC, exception handlers
â”‚       â”œâ”€â”€ gdt.rs                 # GDT, TSS, kernel stacks
â”‚       â”œâ”€â”€ keyboard.rs            # Scancode ring buffer
â”‚       â”œâ”€â”€ pci.rs                 # PCI bus enumeration
â”‚       â”œâ”€â”€ acpi.rs                # ACPI table parsing
â”‚       â”œâ”€â”€ allocator.rs           # Kernel heap init
â”‚       â”œâ”€â”€ security.rs            # LSM framework
â”‚       â”œâ”€â”€ shell.rs               # Kernel shell (async task)
â”‚       â”œâ”€â”€ tty.rs                 # TTY device
â”‚       â”œâ”€â”€ pty.rs                 # Pseudoterminal
â”‚       â”œâ”€â”€ smp.rs                 # SMP AP boot
â”‚       â”œâ”€â”€ elf_dyn.rs             # Dynamic ELF loading
â”‚       â”œâ”€â”€ emulation.rs           # Linux syscall emulation
â”‚       â”œâ”€â”€ selftest.rs            # Self-test framework
â”‚       â”œâ”€â”€ arch/
â”‚       â”‚   â”œâ”€â”€ mod.rs             # Arch trait (10 methods)
â”‚       â”‚   â”œâ”€â”€ arch_x86_64.rs     # x86_64 implementation
â”‚       â”‚   â””â”€â”€ arch_aarch64.rs    # aarch64 implementation (in progress)
â”‚       â”œâ”€â”€ memory/
â”‚       â”‚   â”œâ”€â”€ mod.rs             # Memory init, virt_to_phys
â”‚       â”‚   â”œâ”€â”€ buddy.rs           # Buddy frame allocator
â”‚       â”‚   â”œâ”€â”€ slab.rs            # Slab object allocator
â”‚       â”‚   â”œâ”€â”€ paging.rs          # Page tables (AddressSpace)
â”‚       â”‚   â”œâ”€â”€ frame_info.rs      # Frame tracking
â”‚       â”‚   â””â”€â”€ stack.rs           # Kernel stack allocation
â”‚       â”œâ”€â”€ task/
â”‚       â”‚   â”œâ”€â”€ mod.rs             # Task/YieldNow async primitive
â”‚       â”‚   â”œâ”€â”€ thread.rs          # Thread struct, context switch, userspace jump
â”‚       â”‚   â”œâ”€â”€ process.rs         # Process, ELF loading, VMA, fork/execve
â”‚       â”‚   â”œâ”€â”€ scheduler.rs       # Preemptive scheduler
â”‚       â”‚   â”œâ”€â”€ executor.rs        # Async executor
â”‚       â”‚   â””â”€â”€ keyboard.rs        # Async keyboard queue
â”‚       â”œâ”€â”€ syscalls/
â”‚       â”‚   â”œâ”€â”€ mod.rs             # Syscall dispatch, signals
â”‚       â”‚   â”œâ”€â”€ numbers.rs         # Syscall number constants
â”‚       â”‚   â”œâ”€â”€ errno.rs           # Error numbers
â”‚       â”‚   â”œâ”€â”€ signal.rs          # Signal types/state
â”‚       â”‚   â”œâ”€â”€ user_access.rs     # SMAP-safe user memory access
â”‚       â”‚   â””â”€â”€ io_uring.rs        # io_uring setup/enter
â”‚       â”œâ”€â”€ vfs/
â”‚       â”‚   â”œâ”€â”€ mod.rs             # VFS manager, node/fs traits, mount, path resolution
â”‚       â”‚   â”œâ”€â”€ ramfs.rs           # In-memory tmpfs
â”‚       â”‚   â”œâ”€â”€ devfs.rs           # Device filesystem
â”‚       â”‚   â”œâ”€â”€ ctlfs.rs           # Plan9-style control FS
â”‚       â”‚   â”œâ”€â”€ tarfs.rs           # Read-only tar FS
â”‚       â”‚   â”œâ”€â”€ fat.rs             # FAT32 via fatfs crate
â”‚       â”‚   â”œâ”€â”€ ext2.rs            # ext2 filesystem
â”‚       â”‚   â”œâ”€â”€ pipe.rs            # Unix pipe IPC
â”‚       â”‚   â””â”€â”€ skyfs/             # SkyFS journaling filesystem
â”‚       â”‚       â”œâ”€â”€ mod.rs         # SkyFS superblock, format, mount
â”‚       â”‚       â”œâ”€â”€ alloc.rs       # Block bitmap allocator
â”‚       â”‚       â”œâ”€â”€ btree.rs       # B-tree extent storage
â”‚       â”‚       â”œâ”€â”€ dir.rs         # Directory operations
â”‚       â”‚       â”œâ”€â”€ inode.rs       # Inode read/write
â”‚       â”‚       â””â”€â”€ journal.rs     # WAL journaling
â”‚       â”œâ”€â”€ drivers/
â”‚       â”‚   â”œâ”€â”€ mod.rs             # Driver module declarations
â”‚       â”‚   â”œâ”€â”€ ps2.rs             # PS/2 controller
â”‚       â”‚   â”œâ”€â”€ mouse.rs           # PS/2 mouse
â”‚       â”‚   â”œâ”€â”€ rtc.rs             # Real-time clock
â”‚       â”‚   â”œâ”€â”€ graphics.rs        # UEFI GOP framebuffer
â”‚       â”‚   â”œâ”€â”€ input.rs           # Input subsystem
â”‚       â”‚   â”œâ”€â”€ watchdog.rs        # Watchdog timer
â”‚       â”‚   â”œâ”€â”€ net/
â”‚       â”‚   â”‚   â”œâ”€â”€ mod.rs         # Network module
â”‚       â”‚   â”‚   â”œâ”€â”€ e1000.rs       # Intel E1000 driver
â”‚       â”‚   â”‚   â””â”€â”€ virtio.rs      # VirtIO-Net driver
â”‚       â”‚   â”œâ”€â”€ block/
â”‚       â”‚   â”‚   â”œâ”€â”€ mod.rs         # Block device trait
â”‚       â”‚   â”‚   â”œâ”€â”€ cache.rs       # Block cache
â”‚       â”‚   â”‚   â””â”€â”€ partition.rs   # MBR/GPT partition parser
â”‚       â”‚   â”œâ”€â”€ storage/
â”‚       â”‚   â”‚   â”œâ”€â”€ ahci.rs        # AHCI SATA driver
â”‚       â”‚   â”‚   â”œâ”€â”€ nvme.rs        # NVMe SSD driver
â”‚       â”‚   â”‚   â””â”€â”€ virtio_block.rs # VirtIO-Block driver
â”‚       â”‚   â”œâ”€â”€ gpu/
â”‚       â”‚   â”‚   â””â”€â”€ virtio_gpu.rs  # VirtIO GPU driver
â”‚       â”‚   â”œâ”€â”€ audio/
â”‚       â”‚   â”‚   â”œâ”€â”€ hda.rs         # Intel HDA audio driver
â”‚       â”‚   â”‚   â””â”€â”€ pcspeaker.rs   # PC speaker driver
â”‚       â”‚   â””â”€â”€ usb/
â”‚       â”‚       â””â”€â”€ xhci.rs        # xHCI USB 3.0 driver
â”‚       â”œâ”€â”€ apic/
â”‚       â”‚   â”œâ”€â”€ mod.rs             # APIC module
â”‚       â”‚   â”œâ”€â”€ lapic.rs           # Local APIC
â”‚       â”‚   â””â”€â”€ ioapic.rs          # I/O APIC
â”‚       â”œâ”€â”€ net/
â”‚       â”‚   â”œâ”€â”€ mod.rs             # Network stack (smoltcp)
â”‚       â”‚   â”œâ”€â”€ dhcp.rs            # DHCP client
â”‚       â”‚   â””â”€â”€ dns.rs             # DNS resolver
â”‚       â”œâ”€â”€ gui/
â”‚       â”‚   â”œâ”€â”€ mod.rs             # GUI compositor
â”‚       â”‚   â”œâ”€â”€ window.rs          # Window management
â”‚       â”‚   â”œâ”€â”€ drawing.rs         # Drawing primitives
â”‚       â”‚   â”œâ”€â”€ terminal.rs        # Terminal emulator
â”‚       â”‚   â”œâ”€â”€ splash.rs          # Boot splash screen
â”‚       â”‚   â”œâ”€â”€ shell.rs           # Window manager
â”‚       â”‚   â”œâ”€â”€ filemanager.rs     # File manager widget
â”‚       â”‚   â”œâ”€â”€ mouse.rs           # Mouse cursor
â”‚       â”‚   â”œâ”€â”€ widgets.rs         # Desktop widgets
â”‚       â”‚   â””â”€â”€ wallpaper.rs       # Wallpaper rendering
â”‚       â”œâ”€â”€ ebpf/
â”‚       â”‚   â”œâ”€â”€ mod.rs             # eBPF module
â”‚       â”‚   â”œâ”€â”€ vm.rs              # eBPF virtual machine
â”‚       â”‚   â”œâ”€â”€ verifier.rs        # eBPF verifier
â”‚       â”‚   â”œâ”€â”€ maps.rs            # eBPF maps
â”‚       â”‚   â””â”€â”€ helpers.rs         # Built-in eBPF helpers
â”‚       â”œâ”€â”€ crypto/
â”‚       â”‚   â”œâ”€â”€ mod.rs             # Crypto module
â”‚       â”‚   â””â”€â”€ sha256.rs          # SHA-256 implementation
â”‚       â”œâ”€â”€ debug/
â”‚       â”‚   â”œâ”€â”€ mod.rs             # Debug module
â”‚       â”‚   â””â”€â”€ symbols.rs         # Symbol lookup/unwinding
â”‚       â””â”€â”€ tests/                 # Unit tests (self_test feature)
â”œâ”€â”€ userspace/                     # Userspace workspace
â”‚   â”œâ”€â”€ Cargo.toml                 # 15 workspace members
â”‚   â”œâ”€â”€ init/                      # Init process (PID 1)
â”‚   â”œâ”€â”€ sargash/                   # Shell
â”‚   â”œâ”€â”€ libc/                      # C standard library
â”‚   â”œâ”€â”€ libskyos/                  # OS library
â”‚   â”œâ”€â”€ libsarga/                  # Alt userspace library
â”‚   â”œâ”€â”€ libskyaudio/               # Audio library
â”‚   â”œâ”€â”€ coreutils/                 # 40+ Unix utilities
â”‚   â”œâ”€â”€ skyedit/                   # Text editor
â”‚   â”œâ”€â”€ sarga-disp/                # Display server
â”‚   â”œâ”€â”€ skypkg/                    # Package manager
â”‚   â”œâ”€â”€ login/                     # Login utility
â”‚   â”œâ”€â”€ passwd/                    # Password utility
â”‚   â”œâ”€â”€ skybuild/                  # Build tool
â”‚   â”œâ”€â”€ setup/                     # System setup
â”‚   â”œâ”€â”€ svc/                       # Service manager
â”‚   â””â”€â”€ vahid/                     # Vahi daemon
â”œâ”€â”€ builder/                       # Bootimage builder crate
â”‚   â””â”€â”€ src/main.rs                # Creates UEFI bootable disk image
â”œâ”€â”€ SkyOS/                         # Initrd staging
â”‚   â”œâ”€â”€ bin/                       # Userspace binaries
â”‚   â”œâ”€â”€ etc/                       # Config files
â”‚   â””â”€â”€ initrd.tar                 # Packed initramfs
â”œâ”€â”€ docs/                          # Documentation (23+ files)
â”‚   â”œâ”€â”€ index.md                   # Documentation hub
â”‚   â”œâ”€â”€ ARCHITECTURE.md            # Architecture overview
â”‚   â”œâ”€â”€ BUILD.md                   # Build instructions
â”‚   â”œâ”€â”€ CHANGELOG.md               # Changelog
â”‚   â”œâ”€â”€ CONTRIBUTING.md            # Contributing guide
â”‚   â”œâ”€â”€ DRIVER_MODEL.md            # Driver architecture
â”‚   â”œâ”€â”€ MEMORY_MAP.md              # Virtual address space
â”‚   â”œâ”€â”€ SCHEDULER.md               # Scheduler design
â”‚   â”œâ”€â”€ SYSCALL_ABI.md             # Frozen syscall ABI
â”‚   â”œâ”€â”€ VFS_DESIGN.md              # VFS design
â”‚   â”œâ”€â”€ api/                       # API reference
â”‚   â”œâ”€â”€ architecture/              # Deep architecture dives
â”‚   â”œâ”€â”€ build/                     # Build system docs
â”‚   â”œâ”€â”€ contributing/              # Contribution workflow
â”‚   â”œâ”€â”€ design/                    # Design decisions
â”‚   â”œâ”€â”€ drivers/                   # Driver documentation
â”‚   â”œâ”€â”€ future/                    # Roadmap
â”‚   â”œâ”€â”€ guide/                     # Developer guides
â”‚   â”œâ”€â”€ reference/                 # Technical reference
â”‚   â”œâ”€â”€ security/                  # Security docs
â”‚   â”œâ”€â”€ syscalls/                  # Syscall table
â”‚   â””â”€â”€ testing/                   # Testing methodology
â”œâ”€â”€ tests/                         # Integration tests
â”‚   â”œâ”€â”€ test_boot.ps1              # Boot test
â”‚   â”œâ”€â”€ test_login.ps1             # Login test
â”‚   â””â”€â”€ test_panic.ps1             # Panic test
â”œâ”€â”€ .github/workflows/             # CI pipeline
â”‚   â”œâ”€â”€ build.yml                  # Build + selftest workflow
â”‚   â””â”€â”€ build-kernel.yml           # Kernel build workflow
â”œâ”€â”€ make_bootimage.ps1             # Windows bootimage script
â”œâ”€â”€ make_bootimage.sh              # Linux bootimage script
â”œâ”€â”€ build_userspace.ps1            # Userspace build script
â”œâ”€â”€ build_initrd.py                # Initrd creation script
â”œâ”€â”€ build_disk.py                  # Disk image creation
â”œâ”€â”€ run_qemu_display.ps1           # QEMU launch (display)
â”œâ”€â”€ run_test_nographic.ps1         # QEMU launch (serial-only)
â””â”€â”€ vahi_uefi.img                  # Pre-built disk image
```

---

## Documentation

Comprehensive documentation lives in the [`docs/`](docs/) directory:

| Document | Description |
|----------|-------------|
| [Architecture](docs/ARCHITECTURE.md) | Kernel architecture overview, boot flow, module design |
| [Build Guide](docs/BUILD.md) | Build prerequisites, QEMU setup, VirtualBox |
| [Syscall ABI](docs/SYSCALL_ABI.md) | Frozen syscall ABI specification (v1.0) |
| [Memory Map](docs/MEMORY_MAP.md) | Virtual address space layout |
| [Scheduler](docs/SCHEDULER.md) | Preemptive + cooperative hybrid scheduler design |
| [VFS Design](docs/VFS_DESIGN.md) | VFS traits, filesystem stack, path resolution |
| [Driver Model](docs/DRIVER_MODEL.md) | Character, block, network, PCI driver architecture |
| [Contributing](docs/CONTRIBUTING.md) | PR workflow, code style, testing |
| [Changelog](docs/CHANGELOG.md) | Version history and release notes |

Additional deep-dive directories:

```
docs/
â”œâ”€â”€ api/           # Syscall API reference (read, write, open, mmap, execve, GUI, VFS, drivers, libc)
â”œâ”€â”€ architecture/  # Overview, memory, process, scheduling, interrupts, syscall, SMP, IPC, sync, time
â”œâ”€â”€ build/         # Prerequisites, building, boot images, config, cross-compilation, Docker, troubleshooting
â”œâ”€â”€ contributing/  # Code of conduct, PRs, issues, maintainers, license
â”œâ”€â”€ design/        # Philosophy, why Rust, async model, VFS, memory safety, GUI, networking, driver model, ELF
â”œâ”€â”€ drivers/       # PS/2, mouse, keyboard, graphics, RTC, E1000, VirtIO-Net, PCI, ACPI
â”œâ”€â”€ future/        # 8-phase roadmap (stabilization, networking, GUI, userspace, drivers, security, performance, portability)
â”œâ”€â”€ guide/         # Getting started, QEMU, adding a syscall, writing a driver, debugging, testing, VFS guide
â”œâ”€â”€ reference/     # x86_64, UEFI, ELF, PCI IDs, PS/2 scan codes, I/O ports, IRQ table, memory map
â”œâ”€â”€ security/      # Memory protection, syscall security, user isolation, future security
â”œâ”€â”€ syscalls/      # Individual syscall documentation
â””â”€â”€ testing/       # Unit, integration, memory, syscall, network, stress, regression, CI/CD
```

---

## Testing

### Unit Tests (Self-Test)

Build with the `self_test` feature to run built-in kernel tests:

```bash
cd kernel
cargo build --features self_test
```

The self-test framework covers:
- **SkyFS**: format, mount, create, write, read, unlink, directory operations
- **eBPF verifier**: LDX_R10 protection, CALL helper validation, bad helper rejection

### Integration Tests (QEMU)

PowerShell-based integration tests using QEMU:

```powershell
.\tests\test_boot.ps1     # Boot and wait for login prompt
.\tests\test_login.ps1    # Login with root/root and get shell
.\tests\test_panic.ps1    # Verify kernel panic handling
```

Each test runs QEMU in `-nographic` mode, monitors serial output, and returns PASS/FAIL.

### CI Pipeline

```yaml
# .github/workflows/build.yml
Jobs:
  - build-kernel:   x86_64 + aarch64 compile
  - build-userspace: Userspace compilation
  - selftest:       Self-test feature compilation
```

---

## Architecture Portability

### Arch Trait

The kernel abstracts architecture-specific operations behind the `Arch` trait:

```rust
pub trait Arch: Send + Sync {
    unsafe fn init_boot();
    unsafe fn init_syscalls();
    fn read_sp() -> u64;
    fn read_fp() -> u64;
    fn halt();
    fn halt_loop() -> !;
    unsafe fn jump_to_usermode(entry: u64, rsp: u64) -> !;
    unsafe fn switch_thread(old_sp: *mut u64, new_sp: u64, new_fs_base: u64);
    fn read_thread_pointer() -> u64;
    unsafe fn write_thread_pointer(val: u64);
}
```

### x86_64 (Primary, Mature)

```
- SYSCALL/SYSRET instruction pair for syscall entry/exit
- FSGSBASE (rdfsbase/wrfsbase) for TLS, MSR fallback
- 4-level paging (48-bit virtual address space)
- Higher-half kernel at 0xFFFFFFFF80000000
- UEFI boot via bootloader crate
- Full exception handling: #PF, #GP, #UD, #NM, #SS, #DB, #BP, #DF (IST)
- APIC (LAPIC + I/O APIC)
- SMP via SIPI
```

### aarch64 (In Progress)

```
- _start_aarch64 entry point with BSS clearing
- WFI for halt
- ERET to EL0 for userspace jump
- TPIDR_EL0 for thread pointer
- SVC for syscalls (stub)
- VBAR_EL1 vector table (skeleton)
- GICv2/v3 interrupt controller (stub)
- Generic timer (stub)
- Context switch: x19-x28, x29, x30 save/restore
- 4-level page tables (48-bit VA) with MMU init (stub)
- Kernel loaded at 0x40080000 (QEMU virt DRAM)
- Target spec: aarch64-unknown-none (soft-float, strict-align)
```

---

## Contributing

We welcome contributions under the **SKYIOUS Software License (SSL)**.

### Getting Started

1. Read the [Contribution Guidelines](docs/CONTRIBUTING.md)
2. Check the [Architecture Overview](docs/ARCHITECTURE.md)
3. Review the [Future Roadmap](docs/future/)
4. Set up your [development environment](docs/guide/getting_started.md)

### Development Workflow

```bash
# Build kernel
cd kernel && cargo build

# Run tests
cargo build --features self_test

# Build bootimage
cd ../builder && cargo run -- ../kernel/target/x86_64-unknown-none/debug/vahi_kernel

# Test in QEMU
../run_test_nographic.ps1
```

### Coding Standards

- Rust nightly with `#![deny(warnings)]`
- No panicking paths in interrupt context
- Safe abstractions over unsafe primitives
- Document all public items and unsafe blocks
- Follow existing module patterns
- Test new features with both unit and integration tests

---

## License

**SKYIOUS Software License (SSL) v1.0**

Copyright (c) 2026 SARGA OS Contributors

A file-level copyleft license that balances freedom for users with protection for the original project. See the [LICENSE](LICENSE) file for full terms.

Key provisions:
- Commercial use permitted with attribution
- Attribution required in documentation and UI
- File-level copyleft (modified files must share-alike)
- Patent grant included
- Optional Maintainer Right clause for upstream fork incorporation
- 30-day cure period for license violations
- "or any later version" compatibility

## Testing with a Disk Drive

Create a test disk image and boot the kernel with it attached as a secondary IDE drive:

```powershell
.\test_drive_qemu.ps1
```

This runs `make_disk_image.ps1` to create `test_disk.img` (32 MB, MBR-partitioned) and boots QEMU with both the kernel disk and the test disk attached. The kernel's PCI enumeration detects the IDE controller and probes the PATA/IDE fallback driver (if AHCI is not available) or the AHCI driver directly.

To create just the disk image:

```powershell
.\make_disk_image.ps1 -Size "64M" -OutFile "my_disk.img"
```

To boot with a disk image manually:

```powershell
qemu-system-x86_64 -bios OVMF.fd -drive format=raw,file=skyos_uefi.img,if=ide,index=0 -drive format=raw,file=test_disk.img,if=ide,index=1 -m 512M -smp 2 -serial stdio
```

The kernel's `pata::mbr_signature` self-test reads sector 0 from the first block device and verifies the 0x55AA MBR signature. Check the serial console for test results.
