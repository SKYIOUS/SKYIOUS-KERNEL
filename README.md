# Vahi Kernel

> A modern, monolithic Rust kernel — the core of **SARGA OS**.
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
  - [Korlang Runtime](#korlang-runtime)
  - [VahiAI](#vahiai)
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

**Vahi** (Sanskrit: "the carrier") is a monolithic kernel written entirely in Rust. It powers **SARGA OS** — a modern operating system built from scratch with a focus on safety, performance, and extensibility.

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

---

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                     Vahi Kernel                               │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐  │
│  │                  Syscall Layer                          │  │
│  │  90+ syscalls: read/write/open/mmap/fork/execve/net/   │  │
│  │  gui/clone/futex/io_uring/bpf/vahiai                   │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌────────────┐ ┌──────────┐ ┌────────┐ ┌───────────────┐   │
│  │  Scheduler │ │  Memory  │ │  VFS   │ │  Network      │   │
│  │  Preemptive│ │  Buddy   │ │ 7 FS   │ │  smoltcp      │   │
│  │  8 prio    │ │  Slab    │ │ mounts │ │  E1000/VirtIO │   │
│  └────────────┘ └──────────┘ └────────┘ └───────────────┘   │
│                                                              │
│  ┌────────────┐ ┌──────────┐ ┌────────┐ ┌───────────────┐   │
│  │  Drivers   │ │  GUI     │ │ eBPF   │ │  Security     │   │
│  │  12+ devs  │ │Compositor│ │ VM+Ver │ │  SMEP/UMIP/   │   │
│  │  PCI/ACPI  │ │ 30 FPS   │ │ Map+Hlp│ │  ASLR/Caps    │   │
│  └────────────┘ └──────────┘ └────────┘ └───────────────┘   │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐  │
│  │              Arch Abstraction (Arch trait)              │  │
│  │  x86_64 (SYSCALL/SYSRET, FSGSBASE)                     │  │
│  │  aarch64 (SVC/ERET, TPIDR_EL0, GICv2/v3)              │  │
│  └────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

### Boot Flow

```
UEFI firmware
    │
    ▼
bootloader crate (UEFI boot protocol)
    │
    ▼
kernel_main()
    ├── KASLR init (RDTSC entropy)
    ├── CPUID feature detection (SMEP, UMIP, FSGSBASE)
    ├── Memory init (OffsetPageTable, physical map)
    ├── Framebuffer init (UEFI GOP)
    ├── Frame allocator init (Buddy)
    ├── Heap init (linked_list_allocator @ 0xFFFF_C000_0000_0000)
    ├── GDT + TSS init
    ├── IDT + PIC init (exception handlers, IRQs)
    ├── Syscall init (STAR/LStar/SFMask MSRs, SYSCALL entry)
    ├── ACPI init (RSDP parse, FADT, MADT)
    ├── APIC init (LAPIC + I/O APIC)
    ├── SMP boot (SIPI to APs)
    ├── PS/2 init (keyboard + mouse)
    ├── PCI enumeration (scan bus, init drivers)
    ├── VFS init (mount initrd, devfs, ctlfs, tmpfs, partitions)
    ├── Network init (smoltcp, E1000)
    ├── LSM init
    ├── GUI init (compositor, window manager, desktop)
    ├── Spawn async tasks (kernel shell, GUI refresh, network poll)
    ├── Spawn init_os_task (loads /bin/init into userspace)
    ├── Enable interrupts (sti)
    └── Enter scheduler (never returns)
```

### Memory Layout

```
0x0000_0000_0000 ┌──────────────────────┐
                 │   Userspace          │
                 │   (per-process)      │
0x7FFF_FFFF_E000 ├──────────────────────┤
                 │   Kernel Mapping     │
0xFFFF_8000_0000 ├──────────────────────┤
                 │   Physical Memory    │
                 │   (1:1 mapped)       │
0xFFFF_C000_0000 ├──────────────────────┤
                 │   Kernel Heap        │
                 │   (Buddy + Slab)     │
0xFFFF_FFFF_FFFF └──────────────────────┘
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
| **Korlang** | `korlang` (#201) |
| **AI** | `vahiai` (#300) |

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
| **HDA** | Intel High Definition Audio — playback, volume control (0-100%), stream halt |
| **PC Speaker** | Legacy programmable interval timer beeper |

#### Display

| Driver | Description |
|--------|-------------|
| **UEFI GOP** | Framebuffer from boot services, linear 32bpp |
| **VirtIO GPU** | Para-virtualized GPU with 2D commands, cursor, scanout |

#### USB

| Driver | Description |
|--------|-------------|
| **xHCI** | USB 3.0 controller — device descriptor parsing, config walking, HID/mass storage class detection |

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

The GUI is rendered entirely in kernel space — no userspace display server needed. Each window gets a dedicated framebuffer, and the compositor blends them together at 30 FPS.

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

### Korlang Runtime

```
- Custom programming language runtime
- SYS_KORLANG syscall (#201)
- Korlang interpreter/JIT runtime integration
```

### VahiAI

```
- AI/ML subsystem (gated by ai_rule feature)
- SYS_VAHIAI syscall (#300) for model queries
- Accessible from userspace via aicli / libsarga::vahiai
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
# With display
.\run_qemu_display.ps1

# No display (serial only, for testing)
.\run_test_nographic.ps1
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

---

## Project Structure

```
SKYIOUS KERNEL/
├── kernel/                        # Vahi kernel crate
│   ├── Cargo.toml                 # v0.3.0, nightly Rust
│   ├── rust-toolchain.toml        # nightly, rust-src, llvm-tools
│   ├── build.rs                   # Initrd embedding, hash verification
│   ├── linker.ld                  # x86_64 linker script (higher-half)
│   ├── aarch64-linker.ld          # aarch64 linker script (physical)
│   ├── aarch64-unknown-none.json  # aarch64 target spec
│   └── src/
│       ├── main.rs                # Entry point, boot flow, panic handler
│       ├── vga_buffer.rs          # VGA text-mode driver
│       ├── interrupts.rs          # IDT, PIC, exception handlers
│       ├── gdt.rs                 # GDT, TSS, kernel stacks
│       ├── keyboard.rs            # Scancode ring buffer
│       ├── pci.rs                 # PCI bus enumeration
│       ├── acpi.rs                # ACPI table parsing
│       ├── allocator.rs           # Kernel heap init
│       ├── security.rs            # LSM framework
│       ├── shell.rs               # Kernel shell (async task)
│       ├── tty.rs                 # TTY device
│       ├── pty.rs                 # Pseudoterminal
│       ├── smp.rs                 # SMP AP boot
│       ├── elf_dyn.rs             # Dynamic ELF loading
│       ├── emulation.rs           # Linux syscall emulation
│       ├── selftest.rs            # Self-test framework
│       ├── arch/
│       │   ├── mod.rs             # Arch trait (10 methods)
│       │   ├── arch_x86_64.rs     # x86_64 implementation
│       │   └── arch_aarch64.rs    # aarch64 implementation (in progress)
│       ├── memory/
│       │   ├── mod.rs             # Memory init, virt_to_phys
│       │   ├── buddy.rs           # Buddy frame allocator
│       │   ├── slab.rs            # Slab object allocator
│       │   ├── paging.rs          # Page tables (AddressSpace)
│       │   ├── frame_info.rs      # Frame tracking
│       │   └── stack.rs           # Kernel stack allocation
│       ├── task/
│       │   ├── mod.rs             # Task/YieldNow async primitive
│       │   ├── thread.rs          # Thread struct, context switch, userspace jump
│       │   ├── process.rs         # Process, ELF loading, VMA, fork/execve
│       │   ├── scheduler.rs       # Preemptive scheduler
│       │   ├── executor.rs        # Async executor
│       │   └── keyboard.rs        # Async keyboard queue
│       ├── syscalls/
│       │   ├── mod.rs             # Syscall dispatch, signals
│       │   ├── numbers.rs         # Syscall number constants
│       │   ├── errno.rs           # Error numbers
│       │   ├── signal.rs          # Signal types/state
│       │   ├── user_access.rs     # SMAP-safe user memory access
│       │   └── io_uring.rs        # io_uring setup/enter
│       ├── vfs/
│       │   ├── mod.rs             # VFS manager, node/fs traits, mount, path resolution
│       │   ├── ramfs.rs           # In-memory tmpfs
│       │   ├── devfs.rs           # Device filesystem
│       │   ├── ctlfs.rs           # Plan9-style control FS
│       │   ├── tarfs.rs           # Read-only tar FS
│       │   ├── fat.rs             # FAT32 via fatfs crate
│       │   ├── ext2.rs            # ext2 filesystem
│       │   ├── pipe.rs            # Unix pipe IPC
│       │   └── skyfs/             # SkyFS journaling filesystem
│       │       ├── mod.rs         # SkyFS superblock, format, mount
│       │       ├── alloc.rs       # Block bitmap allocator
│       │       ├── btree.rs       # B-tree extent storage
│       │       ├── dir.rs         # Directory operations
│       │       ├── inode.rs       # Inode read/write
│       │       └── journal.rs     # WAL journaling
│       ├── drivers/
│       │   ├── mod.rs             # Driver module declarations
│       │   ├── ps2.rs             # PS/2 controller
│       │   ├── mouse.rs           # PS/2 mouse
│       │   ├── rtc.rs             # Real-time clock
│       │   ├── graphics.rs        # UEFI GOP framebuffer
│       │   ├── input.rs           # Input subsystem
│       │   ├── watchdog.rs        # Watchdog timer
│       │   ├── net/
│       │   │   ├── mod.rs         # Network module
│       │   │   ├── e1000.rs       # Intel E1000 driver
│       │   │   └── virtio.rs      # VirtIO-Net driver
│       │   ├── block/
│       │   │   ├── mod.rs         # Block device trait
│       │   │   ├── cache.rs       # Block cache
│       │   │   └── partition.rs   # MBR/GPT partition parser
│       │   ├── storage/
│       │   │   ├── ahci.rs        # AHCI SATA driver
│       │   │   ├── nvme.rs        # NVMe SSD driver
│       │   │   └── virtio_block.rs # VirtIO-Block driver
│       │   ├── gpu/
│       │   │   └── virtio_gpu.rs  # VirtIO GPU driver
│       │   ├── audio/
│       │   │   ├── hda.rs         # Intel HDA audio driver
│       │   │   └── pcspeaker.rs   # PC speaker driver
│       │   └── usb/
│       │       └── xhci.rs        # xHCI USB 3.0 driver
│       ├── apic/
│       │   ├── mod.rs             # APIC module
│       │   ├── lapic.rs           # Local APIC
│       │   └── ioapic.rs          # I/O APIC
│       ├── net/
│       │   ├── mod.rs             # Network stack (smoltcp)
│       │   ├── dhcp.rs            # DHCP client
│       │   └── dns.rs             # DNS resolver
│       ├── gui/
│       │   ├── mod.rs             # GUI compositor
│       │   ├── window.rs          # Window management
│       │   ├── drawing.rs         # Drawing primitives
│       │   ├── terminal.rs        # Terminal emulator
│       │   ├── splash.rs          # Boot splash screen
│       │   ├── shell.rs           # Window manager
│       │   ├── filemanager.rs     # File manager widget
│       │   ├── mouse.rs           # Mouse cursor
│       │   ├── widgets.rs         # Desktop widgets
│       │   └── wallpaper.rs       # Wallpaper rendering
│       ├── vahiai/
│       │   └── mod.rs             # VahiAI subsystem
│       ├── korlang/
│       │   ├── mod.rs             # Korlang runtime
│       │   └── runtime.rs         # Korlang interpreter/JIT
│       ├── ebpf/
│       │   ├── mod.rs             # eBPF module
│       │   ├── vm.rs              # eBPF virtual machine
│       │   ├── verifier.rs        # eBPF verifier
│       │   ├── maps.rs            # eBPF maps
│       │   └── helpers.rs         # Built-in eBPF helpers
│       ├── crypto/
│       │   ├── mod.rs             # Crypto module
│       │   └── sha256.rs          # SHA-256 implementation
│       ├── debug/
│       │   ├── mod.rs             # Debug module
│       │   └── symbols.rs         # Symbol lookup/unwinding
│       └── tests/                 # Unit tests (self_test feature)
├── userspace/                     # Userspace workspace
│   ├── Cargo.toml                 # 15 workspace members
│   ├── init/                      # Init process (PID 1)
│   ├── sargash/                   # Shell
│   ├── libc/                      # C standard library
│   ├── libskyos/                  # OS library
│   ├── libsarga/                  # Alt userspace library
│   ├── libskyaudio/               # Audio library
│   ├── coreutils/                 # 40+ Unix utilities
│   ├── skyedit/                   # Text editor
│   ├── sarga-disp/                # Display server
│   ├── skypkg/                    # Package manager
│   ├── login/                     # Login utility
│   ├── passwd/                    # Password utility
│   ├── skybuild/                  # Build tool
│   ├── setup/                     # System setup
│   ├── svc/                       # Service manager
│   └── vahid/                     # Vahi daemon
├── builder/                       # Bootimage builder crate
│   └── src/main.rs                # Creates UEFI bootable disk image
├── SkyOS/                         # Initrd staging
│   ├── bin/                       # Userspace binaries
│   ├── etc/                       # Config files
│   └── initrd.tar                 # Packed initramfs
├── docs/                          # Documentation (23+ files)
│   ├── index.md                   # Documentation hub
│   ├── ARCHITECTURE.md            # Architecture overview
│   ├── BUILD.md                   # Build instructions
│   ├── CHANGELOG.md               # Changelog
│   ├── CONTRIBUTING.md            # Contributing guide
│   ├── DRIVER_MODEL.md            # Driver architecture
│   ├── MEMORY_MAP.md              # Virtual address space
│   ├── SCHEDULER.md               # Scheduler design
│   ├── SYSCALL_ABI.md             # Frozen syscall ABI
│   ├── VFS_DESIGN.md              # VFS design
│   ├── korlang_abi.md             # Korlang ABI
│   ├── api/                       # API reference
│   ├── architecture/              # Deep architecture dives
│   ├── build/                     # Build system docs
│   ├── contributing/              # Contribution workflow
│   ├── design/                    # Design decisions
│   ├── drivers/                   # Driver documentation
│   ├── future/                    # Roadmap
│   ├── guide/                     # Developer guides
│   ├── reference/                 # Technical reference
│   ├── security/                  # Security docs
│   ├── syscalls/                  # Syscall table
│   └── testing/                   # Testing methodology
├── tests/                         # Integration tests
│   ├── test_boot.ps1              # Boot test
│   ├── test_login.ps1             # Login test
│   └── test_panic.ps1             # Panic test
├── .github/workflows/             # CI pipeline
│   ├── build.yml                  # Build + selftest workflow
│   └── build-kernel.yml           # Kernel build workflow
├── make_bootimage.ps1             # Windows bootimage script
├── make_bootimage.sh              # Linux bootimage script
├── build_userspace.ps1            # Userspace build script
├── build_initrd.py                # Initrd creation script
├── build_disk.py                  # Disk image creation
├── run_qemu_display.ps1           # QEMU launch (display)
├── run_test_nographic.ps1         # QEMU launch (serial-only)
└── vahi_uefi.img                  # Pre-built disk image
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
| [Korlang ABI](docs/korlang_abi.md) | Korlang runtime ABI contract |

Additional deep-dive directories:

```
docs/
├── api/           # Syscall API reference (read, write, open, mmap, execve, GUI, VFS, drivers, libc)
├── architecture/  # Overview, memory, process, scheduling, interrupts, syscall, SMP, IPC, sync, time
├── build/         # Prerequisites, building, boot images, config, cross-compilation, Docker, troubleshooting
├── contributing/  # Code of conduct, PRs, issues, maintainers, license
├── design/        # Philosophy, why Rust, async model, VFS, memory safety, GUI, networking, driver model, ELF
├── drivers/       # PS/2, mouse, keyboard, graphics, RTC, E1000, VirtIO-Net, PCI, ACPI
├── future/        # 8-phase roadmap (stabilization, networking, GUI, userspace, drivers, security, performance, portability)
├── guide/         # Getting started, QEMU, adding a syscall, writing a driver, debugging, testing, VFS guide
├── reference/     # x86_64, UEFI, ELF, PCI IDs, PS/2 scan codes, I/O ports, IRQ table, memory map
├── security/      # Memory protection, syscall security, user isolation, future security
├── syscalls/      # Individual syscall documentation
└── testing/       # Unit, integration, memory, syscall, network, stress, regression, CI/CD
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
