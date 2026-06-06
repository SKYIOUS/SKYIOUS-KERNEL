# SARGA OS — Design Q&A (Phase 1)

> 100+ questions answered on May 24, 2026

---

## 1. Identity & Branding

| # | Question | Answer |
|---|----------|--------|
| 1 | Official OS name | **SARGA OS** |
| 2 | OS + kernel relationship | Vahi kernel → SARGA OS — must credit both if used |
| 3 | Shell name | **SargaSH** |
| 4 | GUI toolkit library name | **libsarga** |
| 5 | Desktop environment name | **SkyDE** |
| 6 | Display server name | **sarga-disp** |
| 7 | Device manager name | **vahid** (keep current) |
| 8 | Default hostname | Prompt on install |
| 9 | Boot splash branding | Both Vahi + SARGA combined logo |
| 10 | Boot animation | Animated splash at native GOP resolution |

## 2. Licensing

| # | Question | Answer |
|---|----------|--------|
| 11 | License type | **Write from scratch** — custom license |
| 12 | License requirements | Any use must explicitly credit Vahi kernel and SARGA OS; no small/hidden text |

## 3. Versioning

| # | Question | Answer |
|---|----------|--------|
| 13 | Version format | `MAJOR.MINOR.PATCH (YEAR.MONTH) - Codename` |
| 14 | Starting version | v0.3.0 → will progress toward v1.0 |

## 4. Architecture & Target

| # | Question | Answer |
|---|----------|--------|
| 15 | Target architectures | **Multi-arch**: x86_64 now, ARM64 (aarch64) next, RISC-V later |
| 16 | Max CPU count | **2-4 CPUs** — small multicore |
| 17 | SMP model | Fixed at boot, no CPU hotplug |
| 18 | Installation targets | All platforms: bare metal, VM, cloud images |

## 5. Kernel & Language

| # | Question | Answer |
|---|----------|--------|
| 19 | Kernel language policy | **Rust first** — Rust default, C only when necessary |
| 20 | Rust toolchain | **Stabilize over time** — nightly now, remove unstable features later |
| 21 | Kernel heap strategy | **Best-fit based on all answers** — expand as needed |
| 22 | Memory safety pattern | **Hybrid** — safe wrappers for common patterns, raw for performance |
| 23 | Kernel stack size | **Configurable** at build time |
| 24 | Watchdog timer | **Make optional** — compile-time flag |
| 25 | Debug output | All channels: serial, QEMU debugcon, framebuffer console |
| 26 | Panic behavior | **Panic screen** with diagnostic info (BSOD-style) |
| 27 | Log levels | Simple levels: error/warn/info/debug |
| 28 | Color-coded logs | Yes — color-code kernel messages by severity |
| 29 | Profiling | **Tracing only** — ftrace-style function tracing |
| 30 | Debugger | **Serial debugger** — simple debug shell over serial |
| 31 | Kernel modules | **Partial** — some drivers loadable, core compiled in |

## 6. Scheduling & Concurrency

| # | Question | Answer |
|---|----------|--------|
| 32 | Scheduler type | **Hybrid** — general-purpose default, real-time classes optional |
| 33 | Kernel sync primitives | Keep spin::Mutex for kernel; userspace gets futex-based primitives |
| 34 | IPC (beyond pipe/futex) | **Shared memory** (System V / POSIX shm) |

## 7. Memory Management

| # | Question | Answer |
|---|----------|--------|
| 35 | Virtualization support | **Plan for it** — design namespaces/cgroups for future containers |
| 36 | eBPF support | **Full eBPF** — with verifier, maps, helpers |
| 37 | Default userspace stack | 64KB (keep current), configurable |

## 8. Filesystem & Storage

| # | Question | Answer |
|---|----------|--------|
| 38 | Default filesystem | **Custom SkyFS** — develop SARGA-native filesystem |
| 39 | EFI System Partition | FAT32 now, custom SkyFS later |
| 40 | Disk partitioning | **GPT only** — modern UEFI standard |
| 41 | Initrd compression | **All** — support gzip, zstd, lz4, choose at build |
| 42 | ctlFS scope | **Replace both /proc and /sys** — single unified control FS (Plan9-style) |
| 43 | AHCI NCQ | **Yes** — implement Native Command Queuing |
| 44 | PCI interrupt model | **MSI-X** — essential for NVMe and modern devices |

## 9. Networking

| # | Question | Answer |
|---|----------|--------|
| 45 | Network stack strategy | **Custom stack** — write SkyOS-native TCP/IP stack (long-term) |
| 46 | IPv6 | **Both IPv4 + IPv6 v1** — dual-stack from the start |
| 47 | TCP offload | **Per-driver** — when hardware supports |
| 48 | Network config | **DHCP + static** — DHCP default, static option |
| 49 | Firewall | **v2.0** — packet filter post-release |
| 50 | PXE boot | **v2.0** — future enhancement |
| 51 | WiFi | **Complete** — full wireless support |
| 52 | IO model | **Async kernel IO** — io_uring-style async |

## 10. Drivers & Hardware

| # | Question | Answer |
|---|----------|--------|
| 53 | USB priority | **High priority** — USB keyboard, mouse, storage in v1 |
| 54 | GPU support | **Full GPU stack** — KMS/DRM-like model in future |
| 55 | Input support | **All input** — PS/2, USB HID, touchpads, touchscreens |
| 56 | Audio support | **v1.1 or later** — not a v1.0 blocker |

## 11. Power Management

| # | Question | Answer |
|---|----------|--------|
| 57 | ACPI depth | **Full ACPI** — complete power state support |
| 58 | ACPI AML interpreter | **Yes** — implement AML interpreter |
| 59 | UEFI runtime | **Exit fully** — no UEFI runtime after boot |
| 60 | Device Tree (FDT) | **Both ACPI + FDT** — ACPI for x86, FDT for ARM64 |

## 12. Security

| # | Question | Answer |
|---|----------|--------|
| 61 | Security model | **DAC + capabilities** — Unix perms + POSIX capabilities |
| 62 | Cryptography | **Userspace crypto** — no kernel crypto API |
| 63 | Entropy sources | **All** — RDRAND + IRQ + input + disk timings for /dev/random |
| 64 | Standard | **Custom ABI** — SkyOS-specific ABI, not POSIX |

## 13. Userspace

| # | Question | Answer |
|---|----------|--------|
| 65 | C library target | POSIX.1-2017 compatible musl port |
| 66 | Dynamic linking | Static only v1, dynamic linker v2 |
| 67 | Shell language | **Rust** — SargaSH written in Rust |
| 68 | Coreutils approach | **From scratch** — write every utility in Rust from zero |
| 69 | Init system | **Custom + supervised** — service auto-restart on crash |
| 70 | Tooling style | **Modular binaries** — separate binary per tool |
| 71 | UID/GID ranges | **Custom ranges** — design SkyOS-specific UID range |
| 72 | Default root home | **/root** |
| 73 | Default text editor | **SkyEdit** — custom Rust editor |
| 74 | KorLang role | **Not decided** — evaluate later |

## 14. GUI & Desktop

| # | Question | Answer |
|---|----------|--------|
| 75 | GUI architecture | **Hybrid** — kernel compositor for boot/emergency, userspace for desktop |
| 76 | Display resolution | **UEFI GOP native** — detect from GOP |
| 77 | Font rendering | **Both** — bitmap for console, TTF for GUI |
| 78 | Unicode support | Unicode BMP v1, CJK v2 |
| 79 | Theme format | **TOML config** — theme files in TOML |
| 80 | Window manager style | **All 3, user-switchable** — floating, tiling, full-screen macro |
| 81 | Desktop icons | Yes |

## 15. Package Management

| # | Question | Answer |
|---|----------|--------|
| 82 | Package format | **Completely custom binary format** (.skp) |
| 83 | Package manager | Custom **skypkg** |

## 16. Build & Release

| # | Question | Answer |
|---|----------|--------|
| 84 | CI/CD platform | **GitHub Actions** |
| 85 | Documentation | **All formats** — code docs, design docs, wiki, tutorials |
| 86 | Modularity | **Semi-modular** — some components swappable, core is fixed |
| 87 | Target audience | **All** — powerful, high-perf, minimal, interchangeable, "better than Arch" |
| 88 | "Better than Arch" metrics | **Performance + simplicity** — faster, less memory, simpler config |

## 17. VahiAI & KorLang

| # | Question | Answer |
|---|----------|--------|
| 89 | VahiAI purpose | **Userspace AI lib** — move AI support to userspace, not kernel |
| 90 | KorLang purpose | Not decided — will evaluate later |

## 18. Misc Implementation Details

| # | Question | Answer |
|---|----------|--------|
| 91 | Keyboard layout support | Files in /etc/keymaps/ directory |
| 92 | Logging format | **Binary + JSON** — binary journal + JSON export tools |
| 93 | Root home | /root |
| 94 | Hostname | Prompt on install |
| 95 | Random device | Hybrid — RDRAND + IRQ + input + disk timings |
| 96 | Locking preference | spin::Mutex for kernel, futex for userspace |
| 97 | Kernel panic screen | YES — diagnostic info display |
| 98 | Boot animation | YES — animated at native resolution |
| 99 | Initrd compression | Multi-format support (gzip, zstd, lz4) |
| 100 | First subsystem to implement | **Ask after writing files** |

---

## Summary of Key Design Pillars

| Dimension | Decision |
|-----------|----------|
| **OS Name** | SARGA OS |
| **Kernel** | Vahi (Rust, monolithic, x86_64 → multi-arch) |
| **License** | Custom — mandatory attribution |
| **Version** | Semver + date + codename |
| **Shell** | SargaSH (Rust, custom) |
| **Display** | Hybrid kernel/userspace compositor |
| **FS** | Custom SkyFS (future) |
| **Net stack** | Custom (long-term), smoltcp (short) |
| **Security** | DAC + capabilities |
| **GUI** | libsarga → SkyDE → sarga-disp |
| **Init** | Custom + supervised services |
| **Coreutils** | From scratch in Rust |
| **Pkg mgr** | skypkg with .skp format |
| **Logging** | Binary + JSON journal |
| **Standard** | Custom ABI (not POSIX) |
